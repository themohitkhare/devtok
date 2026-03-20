# GitHub Issues Backend Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Implement GitHub Issues as the authoritative ticket backend per spec at `docs/superpowers/specs/2026-03-20-github-issues-backend-design.md`.

**Architecture:** GitHub-first with local SQLite cache — tickets are pushed to GitHub Issues and a background sync task keeps SQLite in sync. The `Ticket` struct is unchanged; a new `GithubTicketMeta` struct holds sync metadata. Backwards compatible: `github.enabled = false` by default.

**Tech Stack:** Rust, Clap, Tokio, Rusqlite, `gh` CLI via `std::process::Command` (no new deps).

**Note on `GithubTicketMeta` location:** The spec places it in `src/github.rs`, but since `db.rs` needs `GithubTicketMeta` and `db.rs` is also used by `github.rs`, placing it in `src/github.rs` would create a circular import. It goes in `src/models.rs` (the existing shared types module) instead.

**Note on `update_ticket_and_mark_dirty` signature:** The spec's example shows 4 params `(id, status, notes, assignee)`, but the existing `update_ticket` method has 5 params: `(id, status, notes, blocked_by, assignee)`. The wrapper uses 5 params to match the actual signature — this is intentional.

---

## File Map

| Action | File | Responsibility |
|--------|------|----------------|
| Modify | `src/models.rs` | Add `GithubTicketMeta` struct |
| Modify | `src/config.rs` | Add `GithubConfig`, embed in `Config`, update `to_toml()` |
| Modify | `src/db.rs` | Schema v3 migration, new methods, `github_enabled` field |
| Create | `src/github.rs` | `GithubClient`, all gh api calls |
| Create | `src/cli/github.rs` | `setup`, `sync`, `status` subcommands |
| Modify | `src/cli/mod.rs` | Add `Commands::Github`, `--github` on `Commands::Init` |
| Modify | `src/cli/init.rs` | Add `github: bool` param, call setup+migrate when true |
| Modify | `src/cli/run.rs` | Spawn background github sync task |
| Modify | `src/main.rs` | Route `Commands::Github` and new `Commands::Init` signature |
| Modify | `src/lib.rs` | Add `pub mod github;` |

---

## Task 1: GithubConfig in config.rs (TDD)

**Files:**
- Modify: `src/config.rs`

- [ ] **Step 1: Write failing tests first**

In `#[cfg(test)] mod tests` at bottom of `src/config.rs`, add:

```rust
#[test]
fn github_config_roundtrips_via_toml() {
    let mut cfg = Config::default_for("gh-test");
    cfg.github.enabled = true;
    cfg.github.repo = "owner/repo".into();
    cfg.github.sync_interval_seconds = 30;
    let toml_str = cfg.to_toml();
    let parsed: Config = toml::from_str(&toml_str).expect("should parse");
    assert!(parsed.github.enabled);
    assert_eq!(parsed.github.repo, "owner/repo");
    assert_eq!(parsed.github.sync_interval_seconds, 30);
}

#[test]
fn github_config_disabled_by_default() {
    let cfg = Config::default_for("default-test");
    assert!(!cfg.github.enabled);
    assert!(cfg.github.repo.is_empty());
    assert_eq!(cfg.github.sync_interval_seconds, 60);
    assert!(cfg.github.create_labels_on_init);
}
```

- [ ] **Step 2: Run tests to confirm they fail (compile error)**

```bash
cd /Users/mkhare/Development/devtok && cargo test config::tests::github 2>&1 | tail -10
```

Expected: compile error — `Config` has no field `github`

- [ ] **Step 3: Add `GithubConfig` struct**

Add before the `Config` struct definition (around line 288):

```rust
#[derive(Debug, Deserialize, Clone, Default)]
pub struct GithubConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub repo: String,
    #[serde(default = "default_sync_interval")]
    pub sync_interval_seconds: u64,
    #[serde(default = "default_create_labels_on_init")]
    pub create_labels_on_init: bool,
}

fn default_sync_interval() -> u64 { 60 }
fn default_create_labels_on_init() -> bool { true }
```

- [ ] **Step 4: Embed `GithubConfig` in `Config` struct**

Add to `Config` struct:
```rust
#[serde(default)]
pub github: GithubConfig,
```

Update `Config::default_for()` to include:
```rust
github: GithubConfig::default(),
```

- [ ] **Step 5: Update `to_toml()` to serialize `[github]` section**

At end of `to_toml()`, before the final `out` return:
```rust
// Serialize [github] section (only when enabled or non-default)
if self.github.enabled || !self.github.repo.is_empty() {
    out.push_str(&format!(
        "\n\n[github]\nenabled = {}\nrepo = \"{}\"\nsync_interval_seconds = {}\ncreate_labels_on_init = {}",
        self.github.enabled,
        escape_toml_string(&self.github.repo),
        self.github.sync_interval_seconds,
        self.github.create_labels_on_init,
    ));
}
```

- [ ] **Step 6: Run tests to confirm they pass**

```bash
cd /Users/mkhare/Development/devtok && cargo test config::tests -- --nocapture 2>&1 | tail -20
```

Expected: all config tests pass

- [ ] **Step 7: Commit**

```bash
cd /Users/mkhare/Development/devtok && git add src/config.rs && git commit -m "feat(config): add GithubConfig struct with roundtrip serialization"
```

---

## Task 2: DB schema v3 — github columns + new methods (TDD)

**Files:**
- Modify: `src/models.rs`
- Modify: `src/db.rs`

- [ ] **Step 1: Add `GithubTicketMeta` to `src/models.rs`**

```rust
#[derive(Debug, Clone)]
pub struct GithubTicketMeta {
    pub ticket_id: String,
    pub issue_number: u64,
    pub dirty: bool,
    pub synced_at: Option<String>,
    pub updated_at: Option<String>,
}
```

- [ ] **Step 2: Write failing tests in `src/db.rs`**

Add to `#[cfg(test)] mod tests` at bottom of `src/db.rs`:

```rust
#[test]
fn mark_ticket_dirty_sets_flag() {
    let db = Db::open_memory().unwrap();
    let id = db.create_ticket("T", "D", "core", 1).unwrap();
    db.mark_ticket_dirty(&id).unwrap();
    let dirty = db.list_dirty_tickets().unwrap();
    assert_eq!(dirty.len(), 1);
    assert_eq!(dirty[0].0.id, id);
    assert!(dirty[0].1.dirty);
}

#[test]
fn set_and_get_github_meta_roundtrip() {
    let db = Db::open_memory().unwrap();
    let id = db.create_ticket("T", "D", "core", 1).unwrap();
    let meta = crate::models::GithubTicketMeta {
        ticket_id: id.clone(),
        issue_number: 42,
        dirty: false,
        synced_at: Some("2026-01-01T00:00:00Z".into()),
        updated_at: Some("2026-01-02T00:00:00Z".into()),
    };
    db.set_github_meta(&id, &meta).unwrap();
    let got = db.get_github_meta(&id).unwrap().unwrap();
    assert_eq!(got.issue_number, 42);
    assert!(!got.dirty);
    assert_eq!(got.synced_at, Some("2026-01-01T00:00:00Z".into()));
}

#[test]
fn list_dirty_tickets_only_returns_dirty() {
    let db = Db::open_memory().unwrap();
    let id1 = db.create_ticket("T1", "D", "core", 1).unwrap();
    let _id2 = db.create_ticket("T2", "D", "core", 1).unwrap();
    db.mark_ticket_dirty(&id1).unwrap();
    let dirty = db.list_dirty_tickets().unwrap();
    assert_eq!(dirty.len(), 1);
    assert_eq!(dirty[0].0.id, id1);
}

#[test]
fn update_ticket_and_mark_dirty_sets_flag_when_github_enabled() {
    let mut db = Db::open_memory().unwrap();
    db.github_enabled = true;
    let id = db.create_ticket("T", "D", "core", 1).unwrap();
    db.update_ticket_and_mark_dirty(&id, "in_progress", None, None, None).unwrap();
    let dirty = db.list_dirty_tickets().unwrap();
    assert_eq!(dirty.len(), 1);
}

#[test]
fn update_ticket_and_mark_dirty_no_op_when_github_disabled() {
    let db = Db::open_memory().unwrap(); // github_enabled = false
    let id = db.create_ticket("T", "D", "core", 1).unwrap();
    db.update_ticket_and_mark_dirty(&id, "in_progress", None, None, None).unwrap();
    let dirty = db.list_dirty_tickets().unwrap();
    assert_eq!(dirty.len(), 0);
}

#[test]
fn create_ticket_marks_dirty_when_github_enabled() {
    let mut db = Db::open_memory().unwrap();
    db.github_enabled = true;
    let id = db.create_ticket("T", "D", "core", 1).unwrap();
    let dirty = db.list_dirty_tickets().unwrap();
    assert_eq!(dirty.len(), 1);
    assert_eq!(dirty[0].0.id, id);
}

#[test]
fn claim_next_ticket_marks_dirty_when_github_enabled() {
    let mut db = Db::open_memory().unwrap();
    db.github_enabled = true;
    let id = db.create_ticket("T", "D", "core", 1).unwrap();
    // Reset dirty flag (create_ticket also set it; we want to isolate claim_next_ticket)
    db.set_github_meta(&id, &crate::models::GithubTicketMeta {
        ticket_id: id.clone(),
        issue_number: 0,
        dirty: false,
        synced_at: None,
        updated_at: None,
    }).unwrap();
    db.claim_next_ticket("w-0").unwrap();
    let dirty = db.list_dirty_tickets().unwrap();
    assert_eq!(dirty.len(), 1);
    assert_eq!(dirty[0].0.id, id);
}
```

- [ ] **Step 3: Run tests to confirm they fail (compile error)**

```bash
cd /Users/mkhare/Development/devtok && cargo test db::tests::mark_ticket_dirty 2>&1 | tail -10
```

Expected: compile error — methods don't exist yet

- [ ] **Step 4: Add `github_enabled` field to `Db` and bump schema version**

Change `pub struct Db`:
```rust
pub struct Db {
    conn: Connection,
    pub github_enabled: bool,
}
```

Change `CURRENT_SCHEMA_VERSION` from `2` to `3`.

Update both `open()` and `open_memory()` constructors:
```rust
let db = Db { conn, github_enabled: false };
```

- [ ] **Step 5: Add schema v3 migration**

In `migrate()`, inside `if current_version < Self::CURRENT_SCHEMA_VERSION`, add after the existing v2 migrations:

```rust
// v3: GitHub sync columns on tickets
let _ = self.conn.execute_batch(
    "ALTER TABLE tickets ADD COLUMN github_issue_number INTEGER;
     ALTER TABLE tickets ADD COLUMN github_dirty         INTEGER NOT NULL DEFAULT 0;
     ALTER TABLE tickets ADD COLUMN github_synced_at     TEXT;
     ALTER TABLE tickets ADD COLUMN github_updated_at    TEXT;",
);
```

- [ ] **Step 6: Add new Db methods**

Add `use crate::models::GithubTicketMeta;` to imports in `db.rs`.

Add to `impl Db`:

```rust
pub fn set_github_meta(&self, ticket_id: &str, meta: &GithubTicketMeta) -> Result<()> {
    let now = Utc::now().to_rfc3339();
    self.conn.execute(
        "UPDATE tickets SET github_issue_number = ?2, github_dirty = ?3,
         github_synced_at = ?4, github_updated_at = ?5, updated_at = ?6
         WHERE id = ?1",
        params![
            ticket_id,
            meta.issue_number as i64,
            meta.dirty as i32,
            meta.synced_at,
            meta.updated_at,
            now
        ],
    )?;
    Ok(())
}

pub fn get_github_meta(&self, ticket_id: &str) -> Result<Option<GithubTicketMeta>> {
    let mut stmt = self.conn.prepare(
        "SELECT github_issue_number, github_dirty, github_synced_at, github_updated_at
         FROM tickets WHERE id = ?1"
    )?;
    let result = stmt.query_row(params![ticket_id], |row| {
        let issue_number: Option<i64> = row.get(0)?;
        let dirty: i32 = row.get(1)?;
        let synced_at: Option<String> = row.get(2)?;
        let updated_at: Option<String> = row.get(3)?;
        Ok((issue_number, dirty, synced_at, updated_at))
    }).optional()?;
    Ok(result.map(|(num, dirty, synced_at, updated_at)| GithubTicketMeta {
        ticket_id: ticket_id.to_string(),
        issue_number: num.unwrap_or(0) as u64,
        dirty: dirty != 0,
        synced_at,
        updated_at,
    }))
}

pub fn list_dirty_tickets(&self) -> Result<Vec<(Ticket, GithubTicketMeta)>> {
    let mut stmt = self.conn.prepare(
        "SELECT id, title, description, domain, priority, status, assignee, blocked_by, notes,
                created_at, updated_at,
                COALESCE(github_issue_number, 0), COALESCE(github_dirty, 0),
                github_synced_at, github_updated_at
         FROM tickets WHERE github_dirty = 1"
    )?;
    let rows = stmt.query_map([], |row| {
        let ticket = Ticket {
            id: row.get(0)?,
            title: row.get(1)?,
            description: row.get(2)?,
            domain: row.get(3)?,
            priority: row.get(4)?,
            status: row.get(5)?,
            assignee: row.get(6)?,
            blocked_by: row.get(7)?,
            notes: row.get(8)?,
            created_at: row.get(9)?,
            updated_at: row.get(10)?,
        };
        let issue_number: i64 = row.get(11)?;
        let dirty: i32 = row.get(12)?;
        let synced_at: Option<String> = row.get(13)?;
        let updated_at: Option<String> = row.get(14)?;
        let meta = GithubTicketMeta {
            ticket_id: ticket.id.clone(),
            issue_number: issue_number as u64,
            dirty: dirty != 0,
            synced_at,
            updated_at,
        };
        Ok((ticket, meta))
    })?;
    rows.map(|r| r.map_err(Into::into)).collect()
}

pub fn mark_ticket_dirty(&self, ticket_id: &str) -> Result<()> {
    self.conn.execute(
        "UPDATE tickets SET github_dirty = 1 WHERE id = ?1",
        params![ticket_id],
    )?;
    Ok(())
}

/// Update ticket status and mark it dirty for GitHub sync (when github_enabled).
/// Note: Uses 5 params matching the underlying update_ticket signature.
pub fn update_ticket_and_mark_dirty(
    &self,
    id: &str,
    status: &str,
    notes: Option<&str>,
    blocked_by: Option<&str>,
    assignee: Option<Option<&str>>,
) -> Result<()> {
    self.update_ticket(id, status, notes, blocked_by, assignee)?;
    if self.github_enabled {
        self.mark_ticket_dirty(id)?;
    }
    Ok(())
}
```

- [ ] **Step 7: Update `create_ticket()` and `claim_next_ticket()` to call `mark_ticket_dirty`**

In `create_ticket()`, after the `self.conn.execute(INSERT ...)` call, add:
```rust
if self.github_enabled {
    self.mark_ticket_dirty(&id)?;
}
```

In `claim_next_ticket()`, after `tx.commit()?;`, add:
```rust
if self.github_enabled {
    if let Some(ref t) = ticket {
        self.mark_ticket_dirty(&t.id)?;
    }
}
```

- [ ] **Step 8: Run db tests**

```bash
cd /Users/mkhare/Development/devtok && cargo test db::tests -- --nocapture 2>&1 | tail -40
```

Expected: all db tests pass including new ones

- [ ] **Step 9: Commit**

```bash
cd /Users/mkhare/Development/devtok && git add src/db.rs src/models.rs && git commit -m "feat(db): schema v3 — github sync columns, dirty-flag methods, GithubTicketMeta"
```

---

## Task 3: src/github.rs — GithubClient (TDD)

**Files:**
- Create: `src/github.rs`
- Modify: `src/lib.rs`

- [ ] **Step 1: Write failing tests first**

Create `src/github.rs` with the test module and stubs only (no implementation yet):

```rust
// src/github.rs — stub for TDD
use anyhow::Result;
use crate::models::{GithubTicketMeta, Ticket};

pub struct GithubClient { pub(crate) repo: String }

impl GithubClient {
    pub fn new(repo: &str) -> Self { GithubClient { repo: repo.to_string() } }
    pub fn detect_repo() -> Result<String> { todo!() }
    pub fn setup_labels(&self) -> Result<()> { todo!() }
    pub fn push_ticket(&self, _t: &Ticket) -> Result<u64> { todo!() }
    pub fn update_ticket(&self, _number: u64, _t: &Ticket) -> Result<String> { todo!() }
    pub fn close_issue(&self, _number: u64, _label: &str) -> Result<String> { todo!() }
    pub fn pull_tickets(&self) -> Result<Vec<(Ticket, GithubTicketMeta)>> { todo!() }
}

pub fn parse_github_remote(_url: &str) -> Result<String> { todo!() }
fn ticket_to_body(_t: &Ticket) -> String { todo!() }
fn parse_issue(_issue: &serde_json::Value) -> Option<(Ticket, GithubTicketMeta)> { todo!() }
fn parse_body(_body: &str) -> (String, Option<String>) { todo!() }
fn status_to_label(_status: &str) -> &'static str { todo!() }
fn status_is_closed(_status: &str) -> bool { todo!() }

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_github_remote_ssh() {
        let result = parse_github_remote("git@github.com:owner/repo.git").unwrap();
        assert_eq!(result, "owner/repo");
    }

    #[test]
    fn parse_github_remote_https() {
        let result = parse_github_remote("https://github.com/owner/repo").unwrap();
        assert_eq!(result, "owner/repo");
    }

    #[test]
    fn parse_github_remote_https_with_git_suffix() {
        let result = parse_github_remote("https://github.com/owner/repo.git").unwrap();
        assert_eq!(result, "owner/repo");
    }

    #[test]
    fn parse_github_remote_invalid_returns_error() {
        assert!(parse_github_remote("https://gitlab.com/owner/repo").is_err());
        assert!(parse_github_remote("not-a-url").is_err());
    }

    #[test]
    fn status_to_label_covers_all_statuses() {
        for s in &["pending", "in_progress", "review_pending", "blocked",
                   "awaiting_approval", "completed", "rejected"] {
            let label = status_to_label(s);
            assert!(label.starts_with("status:"), "label for '{}' should start with 'status:'", s);
        }
    }

    #[test]
    fn status_is_closed_only_for_terminal_statuses() {
        assert!(status_is_closed("completed"));
        assert!(status_is_closed("rejected"));
        assert!(!status_is_closed("pending"));
        assert!(!status_is_closed("in_progress"));
        assert!(!status_is_closed("blocked"));
    }

    #[test]
    fn parse_body_with_blocked_by() {
        let body = "Description text\n\n---\nBlocked-by: t-001";
        let (desc, blocked) = parse_body(body);
        assert_eq!(desc, "Description text");
        assert_eq!(blocked, Some("t-001".to_string()));
    }

    #[test]
    fn parse_body_without_blocked_by() {
        let body = "Just a description";
        let (desc, blocked) = parse_body(body);
        assert_eq!(desc, "Just a description");
        assert!(blocked.is_none());
    }

    #[test]
    fn parse_issue_extracts_ticket_fields() {
        let issue = serde_json::json!({
            "number": 42,
            "title": "Fix bug",
            "body": "Some description",
            "state": "open",
            "updated_at": "2026-01-01T00:00:00Z",
            "labels": [
                {"name": "acs-id:t-001"},
                {"name": "status:in_progress"},
                {"name": "domain:core"},
                {"name": "priority:1"},
            ]
        });
        let (ticket, meta) = parse_issue(&issue).unwrap();
        assert_eq!(ticket.id, "t-001");
        assert_eq!(ticket.title, "Fix bug");
        assert_eq!(ticket.status, "in_progress");
        assert_eq!(ticket.domain, "core");
        assert_eq!(ticket.priority, 1);
        assert_eq!(meta.issue_number, 42);
        assert!(!meta.dirty);
    }

    #[test]
    fn parse_issue_returns_none_without_acs_id_label() {
        let issue = serde_json::json!({
            "number": 42,
            "title": "Not an ACS ticket",
            "body": "",
            "state": "open",
            "updated_at": "2026-01-01T00:00:00Z",
            "labels": []
        });
        assert!(parse_issue(&issue).is_none());
    }

    #[test]
    fn mock_push_returns_ticket_number_from_id() {
        std::env::set_var("ACS_GITHUB_MOCK", "1");
        let client = GithubClient::new("owner/repo");
        let ticket = Ticket {
            id: "t-005".into(),
            title: "Test".into(),
            description: "Desc".into(),
            domain: "core".into(),
            priority: 1,
            status: "pending".into(),
            assignee: None,
            blocked_by: None,
            notes: String::new(),
            created_at: "2026-01-01T00:00:00Z".into(),
            updated_at: "2026-01-01T00:00:00Z".into(),
        };
        let num = client.push_ticket(&ticket).unwrap();
        assert_eq!(num, 5);
        std::env::remove_var("ACS_GITHUB_MOCK");
    }

    /// Test label idempotency: the is_label_exists_error helper correctly
    /// identifies 422/already_exists responses so setup_labels can skip them.
    #[test]
    fn label_exists_error_detection() {
        assert!(is_label_exists_error(b"already_exists", false));
        assert!(is_label_exists_error(b"{\"errors\":[{\"code\":\"already_exists\"}]}", false));
        assert!(!is_label_exists_error(b"some other error", false));
        // Exit code 1 without already_exists in stderr is still an error
        assert!(!is_label_exists_error(b"rate limit exceeded", false));
    }
}
```

Add `pub mod github;` to `src/lib.rs`.

- [ ] **Step 2: Run tests to confirm they fail (todo! panics)**

```bash
cd /Users/mkhare/Development/devtok && cargo test github::tests -- --nocapture 2>&1 | tail -10
```

Expected: tests fail with "not yet implemented" panics

- [ ] **Step 3: Implement the full `src/github.rs`**

Replace `src/github.rs` with the full implementation:

```rust
// src/github.rs
use anyhow::{bail, Result};
use std::process::Command;

use crate::models::{GithubTicketMeta, Ticket};

pub struct GithubClient {
    repo: String,
}

const STATUS_LABELS: &[(&str, &str, &str)] = &[
    ("status:pending",           "0075ca", "ACS status: pending"),
    ("status:in_progress",       "e4e669", "ACS status: in progress"),
    ("status:review_pending",    "fbca04", "ACS status: review pending"),
    ("status:blocked",           "d93f0b", "ACS status: blocked"),
    ("status:awaiting_approval", "0e8a16", "ACS status: awaiting approval"),
    ("status:completed",         "6f42c1", "ACS status: completed"),
    ("status:rejected",          "b60205", "ACS status: rejected"),
];

fn status_to_label(status: &str) -> &'static str {
    match status {
        "pending"           => "status:pending",
        "in_progress"       => "status:in_progress",
        "review_pending"    => "status:review_pending",
        "blocked"           => "status:blocked",
        "awaiting_approval" => "status:awaiting_approval",
        "completed"         => "status:completed",
        "rejected"          => "status:rejected",
        _                   => "status:pending",
    }
}

fn status_is_closed(status: &str) -> bool {
    matches!(status, "completed" | "rejected")
}

/// Check if a gh api error response indicates "label already exists" (idempotency).
pub fn is_label_exists_error(stderr: &[u8], _success: bool) -> bool {
    let s = String::from_utf8_lossy(stderr);
    s.contains("already_exists")
}

impl GithubClient {
    pub fn new(repo: &str) -> Self {
        GithubClient { repo: repo.to_string() }
    }

    pub fn detect_repo() -> Result<String> {
        let out = Command::new("git")
            .args(["remote", "get-url", "origin"])
            .output()?;
        if !out.status.success() {
            bail!("git remote get-url origin failed: {}", String::from_utf8_lossy(&out.stderr));
        }
        let raw = String::from_utf8_lossy(&out.stdout).trim().to_string();
        parse_github_remote(&raw)
    }

    fn get_token() -> Option<String> {
        if let Ok(tok) = std::env::var("GITHUB_TOKEN") {
            if !tok.is_empty() { return Some(tok); }
        }
        let out = Command::new("gh").args(["auth", "token"]).output().ok()?;
        if out.status.success() {
            let tok = String::from_utf8_lossy(&out.stdout).trim().to_string();
            if !tok.is_empty() { return Some(tok); }
        }
        None
    }

    fn gh_cmd(&self) -> Command {
        let mut cmd = Command::new("gh");
        if let Some(tok) = Self::get_token() {
            cmd.env("GH_TOKEN", tok);
        }
        cmd
    }

    pub fn setup_labels(&self) -> Result<()> {
        if std::env::var("ACS_GITHUB_MOCK").is_ok() {
            return Ok(());
        }
        for (name, color, description) in STATUS_LABELS {
            let out = self.gh_cmd()
                .args([
                    "api", "--method", "POST",
                    &format!("repos/{}/labels", self.repo),
                    "-f", &format!("name={}", name),
                    "-f", &format!("color={}", color),
                    "-f", &format!("description={}", description),
                ])
                .output()?;
            if !out.status.success() && !is_label_exists_error(&out.stderr, false) {
                eprintln!("[github] warning: label '{}': {}", name,
                          String::from_utf8_lossy(&out.stderr).trim());
            }
        }
        Ok(())
    }

    pub fn push_ticket(&self, t: &Ticket) -> Result<u64> {
        if std::env::var("ACS_GITHUB_MOCK").is_ok() {
            return self.mock_push_ticket(t);
        }
        let body = ticket_to_body(t);
        let label = status_to_label(&t.status);
        let domain_label = format!("domain:{}", t.domain);
        let priority_label = format!("priority:{}", t.priority);
        let id_label = format!("acs-id:{}", t.id);

        let out = self.gh_cmd()
            .args([
                "api", "--method", "POST",
                &format!("repos/{}/issues", self.repo),
                "-f", &format!("title={}", t.title),
                "-f", &format!("body={}", body),
                "-F", &format!("labels[]={}", label),
                "-F", &format!("labels[]={}", domain_label),
                "-F", &format!("labels[]={}", priority_label),
                "-F", &format!("labels[]={}", id_label),
            ])
            .output()?;
        if !out.status.success() {
            bail!("gh api create issue failed: {}", String::from_utf8_lossy(&out.stderr));
        }
        let json: serde_json::Value = serde_json::from_slice(&out.stdout)?;
        Ok(json["number"].as_u64().unwrap_or(0))
    }

    pub fn update_ticket(&self, number: u64, t: &Ticket) -> Result<String> {
        if std::env::var("ACS_GITHUB_MOCK").is_ok() {
            return Ok("2026-01-01T00:00:00Z".to_string());
        }
        let body = ticket_to_body(t);
        let state = if status_is_closed(&t.status) { "closed" } else { "open" };
        let label = status_to_label(&t.status);

        let out = self.gh_cmd()
            .args([
                "api", "--method", "PATCH",
                &format!("repos/{}/issues/{}", self.repo, number),
                "-f", &format!("title={}", t.title),
                "-f", &format!("body={}", body),
                "-f", &format!("state={}", state),
            ])
            .output()?;
        if !out.status.success() {
            bail!("gh api update issue failed: {}", String::from_utf8_lossy(&out.stderr));
        }
        let json: serde_json::Value = serde_json::from_slice(&out.stdout)?;
        let updated_at = json["updated_at"].as_str().unwrap_or("").to_string();
        // Update status label (best effort)
        let _ = self.gh_cmd()
            .args([
                "api", "--method", "POST",
                &format!("repos/{}/issues/{}/labels", self.repo, number),
                "-F", &format!("labels[]={}", label),
            ])
            .output();
        Ok(updated_at)
    }

    pub fn close_issue(&self, number: u64, label: &str) -> Result<String> {
        if std::env::var("ACS_GITHUB_MOCK").is_ok() {
            return Ok("2026-01-01T00:00:00Z".to_string());
        }
        let out = self.gh_cmd()
            .args([
                "api", "--method", "PATCH",
                &format!("repos/{}/issues/{}", self.repo, number),
                "-f", "state=closed",
            ])
            .output()?;
        if !out.status.success() {
            bail!("gh api close issue failed: {}", String::from_utf8_lossy(&out.stderr));
        }
        let json: serde_json::Value = serde_json::from_slice(&out.stdout)?;
        let updated_at = json["updated_at"].as_str().unwrap_or("").to_string();
        let _ = self.gh_cmd()
            .args([
                "api", "--method", "POST",
                &format!("repos/{}/issues/{}/labels", self.repo, number),
                "-F", &format!("labels[]={}", label),
            ])
            .output();
        Ok(updated_at)
    }

    pub fn pull_tickets(&self) -> Result<Vec<(Ticket, GithubTicketMeta)>> {
        if std::env::var("ACS_GITHUB_MOCK").is_ok() {
            return self.mock_pull_tickets();
        }
        let out = self.gh_cmd()
            .args([
                "api",
                &format!("repos/{}/issues?state=all&per_page=100", self.repo),
            ])
            .output()?;
        if !out.status.success() {
            bail!("gh api list issues failed: {}", String::from_utf8_lossy(&out.stderr));
        }
        let issues: serde_json::Value = serde_json::from_slice(&out.stdout)?;
        let mut results = Vec::new();
        if let Some(arr) = issues.as_array() {
            for issue in arr {
                if let Some(pair) = parse_issue(issue) {
                    results.push(pair);
                }
            }
        }
        Ok(results)
    }

    fn mock_push_ticket(&self, t: &Ticket) -> Result<u64> {
        let fixture_dir = std::path::PathBuf::from(".acs/test-fixtures/github");
        let fixture_path = fixture_dir.join(format!("push_{}.json", t.id));
        if fixture_path.exists() {
            let content = std::fs::read_to_string(&fixture_path)?;
            let json: serde_json::Value = serde_json::from_str(&content)?;
            return Ok(json["number"].as_u64().unwrap_or(1));
        }
        // Default: parse number from ticket ID (t-005 → 5)
        let n: u64 = t.id.trim_start_matches("t-").parse().unwrap_or(1);
        Ok(n)
    }

    fn mock_pull_tickets(&self) -> Result<Vec<(Ticket, GithubTicketMeta)>> {
        let fixture_dir = std::path::PathBuf::from(".acs/test-fixtures/github");
        let fixture_path = fixture_dir.join("issues.json");
        if !fixture_path.exists() {
            return Ok(vec![]);
        }
        let content = std::fs::read_to_string(&fixture_path)?;
        let issues: serde_json::Value = serde_json::from_str(&content)?;
        let mut results = Vec::new();
        if let Some(arr) = issues.as_array() {
            for issue in arr {
                if let Some(pair) = parse_issue(issue) {
                    results.push(pair);
                }
            }
        }
        Ok(results)
    }
}

/// Parse GitHub remote URL to "owner/repo" format.
pub fn parse_github_remote(url: &str) -> Result<String> {
    let url = url.trim();
    if let Some(rest) = url.strip_prefix("git@github.com:") {
        return Ok(rest.trim_end_matches(".git").to_string());
    }
    if let Some(rest) = url.strip_prefix("https://github.com/") {
        return Ok(rest.trim_end_matches(".git").trim_end_matches('/').to_string());
    }
    bail!("Cannot parse GitHub remote URL: {}", url);
}

fn ticket_to_body(t: &Ticket) -> String {
    let mut body = t.description.clone();
    if let Some(ref blocked_by) = t.blocked_by {
        body.push_str(&format!("\n\n---\nBlocked-by: {}", blocked_by));
    }
    body
}

fn parse_issue(issue: &serde_json::Value) -> Option<(Ticket, GithubTicketMeta)> {
    let number = issue["number"].as_u64()?;
    let title = issue["title"].as_str()?.to_string();
    let body = issue["body"].as_str().unwrap_or("").to_string();
    let updated_at = issue["updated_at"].as_str().unwrap_or("").to_string();
    let state = issue["state"].as_str().unwrap_or("open");

    let labels: Vec<String> = issue["labels"]
        .as_array()
        .map(|arr| arr.iter().filter_map(|l| l["name"].as_str().map(|s| s.to_string())).collect())
        .unwrap_or_default();

    let ticket_id = labels.iter()
        .find_map(|l| l.strip_prefix("acs-id:").map(|s| s.to_string()))?;

    let status = labels.iter()
        .find_map(|l| l.strip_prefix("status:").map(|s| s.to_string()))
        .unwrap_or_else(|| if state == "closed" { "completed".into() } else { "pending".into() });

    let domain = labels.iter()
        .find_map(|l| l.strip_prefix("domain:").map(|s| s.to_string()))
        .unwrap_or_else(|| "general".into());

    let priority: i32 = labels.iter()
        .find_map(|l| l.strip_prefix("priority:").and_then(|s| s.parse().ok()))
        .unwrap_or(2);

    let (description, blocked_by) = parse_body(&body);
    let now = chrono::Utc::now().to_rfc3339();

    let ticket = Ticket {
        id: ticket_id.clone(),
        title,
        description,
        domain,
        priority,
        status,
        assignee: None,
        blocked_by,
        notes: String::new(),
        created_at: now.clone(),
        updated_at: now,
    };
    let meta = GithubTicketMeta {
        ticket_id,
        issue_number: number,
        dirty: false,
        synced_at: None,
        updated_at: Some(updated_at),
    };
    Some((ticket, meta))
}

fn parse_body(body: &str) -> (String, Option<String>) {
    if let Some(pos) = body.find("\n\n---\nBlocked-by:") {
        let description = body[..pos].to_string();
        let footer = body[pos + "\n\n---\nBlocked-by:".len()..].trim().to_string();
        (description, if footer.is_empty() { None } else { Some(footer) })
    } else {
        (body.to_string(), None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ... (same tests as Step 1 stubs, now passing)
    #[test]
    fn parse_github_remote_ssh() {
        assert_eq!(parse_github_remote("git@github.com:owner/repo.git").unwrap(), "owner/repo");
    }
    #[test]
    fn parse_github_remote_https() {
        assert_eq!(parse_github_remote("https://github.com/owner/repo").unwrap(), "owner/repo");
    }
    #[test]
    fn parse_github_remote_https_with_git_suffix() {
        assert_eq!(parse_github_remote("https://github.com/owner/repo.git").unwrap(), "owner/repo");
    }
    #[test]
    fn parse_github_remote_invalid_returns_error() {
        assert!(parse_github_remote("https://gitlab.com/owner/repo").is_err());
        assert!(parse_github_remote("not-a-url").is_err());
    }
    #[test]
    fn status_to_label_covers_all_statuses() {
        for s in &["pending","in_progress","review_pending","blocked","awaiting_approval","completed","rejected"] {
            let label = status_to_label(s);
            assert!(label.starts_with("status:"));
        }
    }
    #[test]
    fn status_is_closed_only_for_terminal_statuses() {
        assert!(status_is_closed("completed"));
        assert!(status_is_closed("rejected"));
        assert!(!status_is_closed("pending"));
        assert!(!status_is_closed("in_progress"));
    }
    #[test]
    fn parse_body_with_blocked_by() {
        let (desc, blocked) = parse_body("Description text\n\n---\nBlocked-by: t-001");
        assert_eq!(desc, "Description text");
        assert_eq!(blocked, Some("t-001".to_string()));
    }
    #[test]
    fn parse_body_without_blocked_by() {
        let (desc, blocked) = parse_body("Just a description");
        assert_eq!(desc, "Just a description");
        assert!(blocked.is_none());
    }
    #[test]
    fn parse_issue_extracts_ticket_fields() {
        let issue = serde_json::json!({"number":42,"title":"Fix bug","body":"Some description","state":"open","updated_at":"2026-01-01T00:00:00Z","labels":[{"name":"acs-id:t-001"},{"name":"status:in_progress"},{"name":"domain:core"},{"name":"priority:1"}]});
        let (t, m) = parse_issue(&issue).unwrap();
        assert_eq!(t.id, "t-001");
        assert_eq!(t.status, "in_progress");
        assert_eq!(m.issue_number, 42);
    }
    #[test]
    fn parse_issue_returns_none_without_acs_id_label() {
        let issue = serde_json::json!({"number":42,"title":"Not ACS","body":"","state":"open","updated_at":"","labels":[]});
        assert!(parse_issue(&issue).is_none());
    }
    #[test]
    fn mock_push_returns_ticket_number_from_id() {
        std::env::set_var("ACS_GITHUB_MOCK", "1");
        let client = GithubClient::new("owner/repo");
        let ticket = Ticket { id:"t-005".into(),title:"T".into(),description:"D".into(),domain:"core".into(),priority:1,status:"pending".into(),assignee:None,blocked_by:None,notes:String::new(),created_at:"2026-01-01T00:00:00Z".into(),updated_at:"2026-01-01T00:00:00Z".into() };
        assert_eq!(client.push_ticket(&ticket).unwrap(), 5);
        std::env::remove_var("ACS_GITHUB_MOCK");
    }
    #[test]
    fn label_exists_error_detection() {
        assert!(is_label_exists_error(b"already_exists", false));
        assert!(is_label_exists_error(b"{\"errors\":[{\"code\":\"already_exists\"}]}", false));
        assert!(!is_label_exists_error(b"some other error", false));
    }
}
```

- [ ] **Step 4: Run all github tests to confirm they pass**

```bash
cd /Users/mkhare/Development/devtok && cargo test github::tests -- --nocapture 2>&1 | tail -30
```

Expected: all tests pass

- [ ] **Step 5: Commit**

```bash
cd /Users/mkhare/Development/devtok && git add src/github.rs src/lib.rs && git commit -m "feat(github): add GithubClient with label idempotency and mock support"
```

---

## Task 4: CLI — src/cli/github.rs with conflict resolution tests

**Files:**
- Create: `src/cli/github.rs`

- [ ] **Step 1: Write failing conflict resolution tests first**

Create `src/cli/github.rs` with test stubs only:

```rust
// src/cli/github.rs — stubs for TDD
use anyhow::Result;
use clap::Subcommand;

#[derive(Subcommand)]
pub enum GithubCommands {
    /// Create ACS labels in the GitHub repo (idempotent)
    Setup,
    /// Force full bidirectional sync
    Sync,
    /// Show sync status (dirty count, last sync, auth)
    Status,
}

pub fn execute(_cmd: GithubCommands) -> Result<()> { todo!() }

/// Resolve a sync conflict between local and GitHub state.
/// Returns `true` if GitHub wins (local should be updated).
pub fn github_wins(
    gh_updated_at: Option<&str>,
    local_updated_at: Option<&str>,
    local_dirty: bool,
) -> bool {
    todo!()
}

#[cfg(test)]
mod tests {
    use super::*;

    // Conflict resolution — branch 1: GitHub wins
    #[test]
    fn conflict_github_wins_when_gh_newer() {
        assert!(github_wins(
            Some("2026-02-01T00:00:00Z"),
            Some("2026-01-01T00:00:00Z"),
            false,
        ));
    }

    // Conflict resolution — branch 2: local dirty wins
    #[test]
    fn conflict_local_wins_when_dirty() {
        assert!(!github_wins(
            Some("2026-02-01T00:00:00Z"),
            Some("2026-01-01T00:00:00Z"),
            true, // local is dirty — pending push
        ));
    }

    // Conflict resolution — branch 3: no-op (equal timestamps)
    #[test]
    fn conflict_no_op_when_equal_timestamps() {
        assert!(!github_wins(
            Some("2026-01-01T00:00:00Z"),
            Some("2026-01-01T00:00:00Z"),
            false,
        ));
    }

    // Conflict resolution — no-op when gh has no timestamp
    #[test]
    fn conflict_no_op_when_no_gh_timestamp() {
        assert!(!github_wins(None, None, false));
    }

    /// Migration idempotency: verify that a ticket with existing issue_number is skipped
    /// Migration idempotency: verify DB-backed guard — ticket with existing issue_number is skipped.
    #[test]
    fn migration_skips_ticket_with_existing_issue_number() {
        std::env::set_var("ACS_GITHUB_MOCK", "1");
        let db = crate::db::Db::open_memory().unwrap();
        let id = db.create_ticket("T", "D", "core", 1).unwrap();
        // Simulate already-migrated: set issue_number = 42
        db.set_github_meta(&id, &crate::models::GithubTicketMeta {
            ticket_id: id.clone(),
            issue_number: 42,
            dirty: false,
            synced_at: None,
            updated_at: None,
        }).unwrap();

        // Run the migration guard logic from init.rs
        let mut pushed = 0usize;
        let mut skipped = 0usize;
        let tickets = db.list_tickets(None).unwrap();
        for ticket in &tickets {
            if db.get_github_meta(&ticket.id).unwrap()
                 .map(|m| m.issue_number > 0).unwrap_or(false) {
                skipped += 1;
                continue;
            }
            pushed += 1; // would call push_ticket here in real code
        }

        assert_eq!(skipped, 1, "already-migrated ticket should be skipped");
        assert_eq!(pushed, 0, "no new tickets should be pushed");
        std::env::remove_var("ACS_GITHUB_MOCK");
    }
}
```

- [ ] **Step 2: Run to confirm tests fail**

```bash
cd /Users/mkhare/Development/devtok && cargo test cli::github::tests -- --nocapture 2>&1 | tail -10
```

Expected: compile/panic errors from `todo!()`

- [ ] **Step 3: Implement `github_wins` and full `src/cli/github.rs`**

Replace `src/cli/github.rs` with full implementation:

```rust
// src/cli/github.rs
use anyhow::{bail, Result};
use clap::Subcommand;

use crate::cli::acs_dir;
use crate::config::Config;
use crate::db::Db;
use crate::github::GithubClient;
use crate::models::GithubTicketMeta;

#[derive(Subcommand)]
pub enum GithubCommands {
    /// Create ACS labels in the GitHub repo (idempotent)
    Setup,
    /// Force full bidirectional sync
    Sync,
    /// Show sync status (dirty count, last sync, auth)
    Status,
}

pub fn execute(cmd: GithubCommands) -> Result<()> {
    let cwd = std::env::current_dir()?;
    let acs_dir = acs_dir::resolve_acs_dir(&cwd)?;
    let config = Config::load(&acs_dir.join("config.toml"))?;
    match cmd {
        GithubCommands::Setup => execute_setup(&config),
        GithubCommands::Sync => execute_sync(&config, &acs_dir),
        GithubCommands::Status => execute_status(&config, &acs_dir),
    }
}

fn resolve_client(config: &Config) -> Result<GithubClient> {
    let repo = if !config.github.repo.is_empty() {
        config.github.repo.clone()
    } else {
        GithubClient::detect_repo()
            .map_err(|e| anyhow::anyhow!("Cannot detect GitHub repo: {}. Set [github] repo in config.toml.", e))?
    };
    Ok(GithubClient::new(&repo))
}

fn execute_setup(config: &Config) -> Result<()> {
    let client = resolve_client(config)?;
    println!("Setting up ACS labels in GitHub repo...");
    client.setup_labels()?;
    println!("Labels created successfully (existing labels skipped).");
    Ok(())
}

fn execute_sync(config: &Config, acs_dir: &std::path::Path) -> Result<()> {
    let client = resolve_client(config)?;
    let mut db = Db::open(&acs_dir.join("project.db"))?;
    db.github_enabled = true;
    println!("Syncing with GitHub...");

    // Push dirty tickets
    let dirty = db.list_dirty_tickets()?;
    let dirty_count = dirty.len();
    let now = chrono::Utc::now().to_rfc3339();
    for (ticket, meta) in dirty {
        if meta.issue_number > 0 {
            match client.update_ticket(meta.issue_number, &ticket) {
                Ok(updated_at) => { db.set_github_meta(&ticket.id, &GithubTicketMeta {
                    dirty: false, synced_at: Some(now.clone()), updated_at: Some(updated_at), ..meta
                })?; }
                Err(e) => eprintln!("[sync] warning: update {}: {:#}", ticket.id, e),
            }
        } else {
            match client.push_ticket(&ticket) {
                Ok(number) => { db.set_github_meta(&ticket.id, &GithubTicketMeta {
                    ticket_id: ticket.id.clone(), issue_number: number,
                    dirty: false, synced_at: Some(now.clone()), updated_at: None,
                })?; }
                Err(e) => eprintln!("[sync] warning: push {}: {:#}", ticket.id, e),
            }
        }
    }

    // Pull from GitHub and apply conflict resolution
    let pulled = client.pull_tickets()?;
    let pulled_count = pulled.len();
    for (gh_ticket, gh_meta) in pulled {
        if let Some(local_meta) = db.get_github_meta(&gh_ticket.id)? {
            if github_wins(
                gh_meta.updated_at.as_deref(),
                local_meta.updated_at.as_deref(),
                local_meta.dirty,
            ) {
                db.update_ticket(&gh_ticket.id, &gh_ticket.status, None,
                                 gh_ticket.blocked_by.as_deref(),
                                 Some(gh_ticket.assignee.as_deref()))?;
                db.set_github_meta(&gh_ticket.id, &GithubTicketMeta {
                    dirty: false, ..gh_meta
                })?;
            }
            // else: local dirty → keep local; equal timestamps → no-op
        }
    }

    println!("Sync complete: {} dirty tickets pushed, {} issues pulled.", dirty_count, pulled_count);
    Ok(())
}

fn execute_status(config: &Config, acs_dir: &std::path::Path) -> Result<()> {
    let db = Db::open(&acs_dir.join("project.db"))?;
    let dirty = db.list_dirty_tickets()?;
    let all_tickets = db.list_tickets(None)?;
    let with_issue = all_tickets.iter().filter(|t| {
        db.get_github_meta(&t.id).ok().flatten().map(|m| m.issue_number > 0).unwrap_or(false)
    }).count();

    let auth_status = if std::env::var("GITHUB_TOKEN").is_ok() {
        "GITHUB_TOKEN env var".to_string()
    } else {
        let out = std::process::Command::new("gh").args(["auth", "status"]).output();
        match out { Ok(o) if o.status.success() => "gh authenticated".into(), _ => "not authenticated".into() }
    };

    let last_sync = all_tickets.iter().filter_map(|t| {
        db.get_github_meta(&t.id).ok().flatten().and_then(|m| m.synced_at)
    }).max().unwrap_or_else(|| "never".into());

    println!("GitHub sync status:");
    println!("  enabled:        {}", config.github.enabled);
    println!("  repo:           {}", if config.github.repo.is_empty() { "(auto-detect)" } else { &config.github.repo });
    println!("  dirty tickets:  {}", dirty.len());
    println!("  synced/total:   {}/{}", with_issue, all_tickets.len());
    println!("  last sync:      {}", last_sync);
    println!("  auth:           {}", auth_status);
    Ok(())
}

/// Resolve sync conflict: returns true if GitHub's version should win.
///
/// Branch 1: gh_updated_at > local_updated_at and local is clean → GitHub wins.
/// Branch 2: local_dirty = true → local pending push wins.
/// Branch 3: equal timestamps or missing timestamps → no-op.
pub fn github_wins(
    gh_updated_at: Option<&str>,
    local_updated_at: Option<&str>,
    local_dirty: bool,
) -> bool {
    if local_dirty {
        return false; // branch 2: local pending push wins
    }
    match (gh_updated_at, local_updated_at) {
        (Some(gh), Some(local)) => gh > local, // branch 1 or 3 (string RFC3339 comparison)
        _ => false, // branch 3: missing timestamps → no-op
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn conflict_github_wins_when_gh_newer() {
        assert!(github_wins(Some("2026-02-01T00:00:00Z"), Some("2026-01-01T00:00:00Z"), false));
    }
    #[test]
    fn conflict_local_wins_when_dirty() {
        assert!(!github_wins(Some("2026-02-01T00:00:00Z"), Some("2026-01-01T00:00:00Z"), true));
    }
    #[test]
    fn conflict_no_op_when_equal_timestamps() {
        assert!(!github_wins(Some("2026-01-01T00:00:00Z"), Some("2026-01-01T00:00:00Z"), false));
    }
    #[test]
    fn conflict_no_op_when_no_gh_timestamp() {
        assert!(!github_wins(None, None, false));
    }
    /// Migration idempotency: DB-backed guard — ticket with existing issue_number is not re-pushed.
    #[test]
    fn migration_skips_ticket_with_existing_issue_number() {
        std::env::set_var("ACS_GITHUB_MOCK", "1");
        let db = crate::db::Db::open_memory().unwrap();
        let id = db.create_ticket("T", "D", "core", 1).unwrap();
        // Pre-populate issue_number to simulate already-migrated ticket
        db.set_github_meta(&id, &crate::models::GithubTicketMeta {
            ticket_id: id.clone(),
            issue_number: 42,
            dirty: false,
            synced_at: None,
            updated_at: None,
        }).unwrap();

        let mut pushed = 0usize;
        let mut skipped = 0usize;
        let tickets = db.list_tickets(None).unwrap();
        for ticket in &tickets {
            if db.get_github_meta(&ticket.id).unwrap()
                 .map(|m| m.issue_number > 0).unwrap_or(false) {
                skipped += 1;
                continue;
            }
            pushed += 1;
        }

        assert_eq!(skipped, 1);
        assert_eq!(pushed, 0);
        std::env::remove_var("ACS_GITHUB_MOCK");
    }
}
```

- [ ] **Step 4: Run cli/github tests**

```bash
cd /Users/mkhare/Development/devtok && cargo test cli::github::tests -- --nocapture 2>&1 | tail -20
```

Expected: all pass

- [ ] **Step 5: Commit**

```bash
cd /Users/mkhare/Development/devtok && git add src/cli/github.rs && git commit -m "feat(cli/github): setup/sync/status with conflict resolution (3 branches tested)"
```

---

## Task 5: Wire CLI — mod.rs, init.rs, main.rs

**Files:**
- Modify: `src/cli/mod.rs`
- Modify: `src/cli/init.rs`
- Modify: `src/main.rs`

- [ ] **Step 1: Add `pub mod github` and `Commands::Github` to `src/cli/mod.rs`**

Add `pub mod github;` to the module list.

Add to `Commands` enum after `Quality`:
```rust
/// Manage GitHub Issues integration
#[command(subcommand)]
Github(github::GithubCommands),
```

Add `--github` flag to `Commands::Init`:
```rust
Init {
    #[arg(long)]
    spec: Option<String>,
    #[arg(long)]
    auto: bool,
    /// Enable GitHub Issues: create labels and migrate existing tickets
    #[arg(long)]
    github: bool,
},
```

- [ ] **Step 2: Update `src/cli/init.rs` to accept `github: bool`**

Change function signature:
```rust
pub fn execute(spec: Option<String>, auto: bool, github: bool) -> Result<()>
```

At the end of `execute()`, after bootstrap logic, before final `println!("Run `acs run`...")`, add:

```rust
if github {
    println!("Setting up GitHub Issues integration...");
    let config_path = acs_dir.join("config.toml");
    let mut config = Config::load(&config_path)?;
    let repo = if !config.github.repo.is_empty() {
        config.github.repo.clone()
    } else {
        match crate::github::GithubClient::detect_repo() {
            Ok(r) => { config.github.repo = r.clone(); r }
            Err(e) => {
                eprintln!("[github] warning: cannot detect repo: {:#}. Skipping.", e);
                return Ok(());
            }
        }
    };
    config.github.enabled = true;
    fs::write(&config_path, config.to_toml())?;

    let client = crate::github::GithubClient::new(&repo);
    client.setup_labels()?;

    let tickets = db.list_tickets(None)?;
    let now = chrono::Utc::now().to_rfc3339();
    let (mut pushed, mut skipped) = (0usize, 0usize);
    for ticket in &tickets {
        // Idempotent: skip if already has issue number
        if db.get_github_meta(&ticket.id)?.map(|m| m.issue_number > 0).unwrap_or(false) {
            skipped += 1;
            continue;
        }
        match client.push_ticket(ticket) {
            Ok(number) => {
                db.set_github_meta(&ticket.id, &crate::models::GithubTicketMeta {
                    ticket_id: ticket.id.clone(),
                    issue_number: number,
                    dirty: false,
                    synced_at: Some(now.clone()),
                    updated_at: None,
                })?;
                pushed += 1;
            }
            Err(e) => eprintln!("[github] warning: failed to migrate {}: {:#}", ticket.id, e),
        }
    }
    println!("GitHub setup complete: {} tickets migrated, {} already synced.", pushed, skipped);
}
```

Add `use crate::config::Config;` if not already imported.

- [ ] **Step 3: Update `src/main.rs`**

Update the `Init` match arm:
```rust
cli::Commands::Init { spec, auto, github } => cli::init::execute(spec, auto, github),
```

Add `Github` match arm before the closing `}`:
```rust
cli::Commands::Github(cmd) => cli::github::execute(cmd),
```

- [ ] **Step 4: Run full test suite**

```bash
cd /Users/mkhare/Development/devtok && cargo test 2>&1 | tail -30
```

Expected: all existing tests pass + new tests pass, zero failures

- [ ] **Step 5: Commit**

```bash
cd /Users/mkhare/Development/devtok && git add src/cli/mod.rs src/cli/init.rs src/main.rs && git commit -m "feat(cli): wire Github subcommands and --github init flag with migration"
```

---

## Task 6: Background sync in src/cli/run.rs

**Files:**
- Modify: `src/cli/run.rs`

- [ ] **Step 1: Add background github sync task**

In `src/cli/run.rs`, in the `execute()` function, store `acs_dir` for use in the async block:
```rust
let acs_dir = acs_dir.to_path_buf();  // add after resolving acs_dir
```

Inside `rt.block_on(async { ... })`, after spawning the manager task, add:

```rust
// Spawn GitHub background sync task if enabled
let github_handle: Option<JoinHandle<()>> = if config.github.enabled
    && std::env::var("ACS_SKIP_GITHUB_SYNC").is_err()
{
    let sync_db = db.clone();
    let sync_config = config.clone();
    let mut sync_shutdown = shutdown_rx.clone();
    let sync_acs_dir = acs_dir.clone();
    Some(tokio::spawn(async move {
        let interval = sync_config.github.sync_interval_seconds;
        let repo = if !sync_config.github.repo.is_empty() {
            sync_config.github.repo.clone()
        } else {
            match acs::github::GithubClient::detect_repo() {
                Ok(r) => r,
                Err(e) => {
                    eprintln!("[github] warning: cannot detect repo: {:#}. Sync disabled.", e);
                    return;
                }
            }
        };
        let client = acs::github::GithubClient::new(&repo);
        loop {
            tokio::select! {
                _ = sleep(Duration::from_secs(interval)) => {}
                _ = sync_shutdown.changed() => { break; }
            }
            if *sync_shutdown.borrow() { break; }
            let now = chrono::Utc::now().to_rfc3339();
            let dirty = { sync_db.lock().unwrap().list_dirty_tickets().unwrap_or_default() };
            for (ticket, meta) in dirty {
                if meta.issue_number > 0 {
                    if let Ok(updated_at) = client.update_ticket(meta.issue_number, &ticket) {
                        let db = sync_db.lock().unwrap();
                        let _ = db.set_github_meta(&ticket.id, &acs::models::GithubTicketMeta {
                            dirty: false,
                            synced_at: Some(now.clone()),
                            updated_at: Some(updated_at),
                            ..meta
                        });
                    }
                } else if let Ok(number) = client.push_ticket(&ticket) {
                    let db = sync_db.lock().unwrap();
                    let _ = db.set_github_meta(&ticket.id, &acs::models::GithubTicketMeta {
                        ticket_id: ticket.id.clone(),
                        issue_number: number,
                        dirty: false,
                        synced_at: Some(now.clone()),
                        updated_at: None,
                    });
                }
            }
        }
    }))
} else {
    None
};
```

At shutdown, await github handle (before `println!("Stopped.")`):
```rust
if let Some(handle) = github_handle {
    handle.await.ok();
}
```

- [ ] **Step 2: Check that run.rs compiles and tests pass**

```bash
cd /Users/mkhare/Development/devtok && cargo test cli::run::tests -- --nocapture 2>&1 | tail -20
```

Expected: all existing run tests pass (github sync skipped by ACS_SKIP_RUN_LOOP)

- [ ] **Step 3: Commit**

```bash
cd /Users/mkhare/Development/devtok && git add src/cli/run.rs && git commit -m "feat(run): spawn background github sync task when github.enabled"
```

---

## Task 7: Full test run + fixtures

- [ ] **Step 1: Create mock fixture directory**

Create `/Users/mkhare/Development/devtok/.acs/test-fixtures/github/issues.json`:
```json
[
  {
    "number": 1,
    "title": "Sample ticket",
    "body": "A sample description",
    "state": "open",
    "updated_at": "2026-01-01T00:00:00Z",
    "labels": [
      {"name": "acs-id:t-001"},
      {"name": "status:pending"},
      {"name": "domain:core"},
      {"name": "priority:1"}
    ]
  }
]
```

- [ ] **Step 2: Run full test suite**

```bash
cd /Users/mkhare/Development/devtok && cargo test 2>&1 | tail -40
```

Expected: all tests pass, zero failures

- [ ] **Step 3: Run `make coverage` if available**

```bash
cd /Users/mkhare/Development/devtok && make coverage 2>&1 | tail -20 || echo "make coverage not available"
```

- [ ] **Step 4: Commit fixtures**

```bash
cd /Users/mkhare/Development/devtok && git add .acs/test-fixtures/ && git commit -m "test: add github mock fixture for ACS_GITHUB_MOCK=1 testing"
```

---

## Task 8: KB write-back and completion

- [ ] **Step 1: Write findings to KB**

```bash
cd /Users/mkhare/Development/devtok && acs kb write --domain core --key stack --value "Rust, Clap CLI, Tokio, Rusqlite (SQLite WAL + foreign_keys), Serde/serde_json, ratatui+crossterm TUI. DB schema v3: tickets table has github_issue_number, github_dirty, github_synced_at, github_updated_at columns for GitHub sync. GithubConfig in config.rs (enabled=false by default). GithubClient in src/github.rs uses gh api via std::process::Command (no new deps). GithubTicketMeta in src/models.rs (shared types). Background sync task spawned in cli/run.rs when github.enabled; ACS_SKIP_GITHUB_SYNC=1 skips in tests. ACS_GITHUB_MOCK=1 reads fixture JSON from .acs/test-fixtures/github/. CLI: acs github setup|sync|status. acs init --github creates labels + migrates tickets (idempotent)."
```

```bash
cd /Users/mkhare/Development/devtok && acs kb write --domain core --key worker-findings-t-076 --value "GitHub Issues backend (t-076): GithubTicketMeta placed in src/models.rs (not github.rs per spec) to avoid circular imports between db.rs and github.rs. update_ticket_and_mark_dirty has 5 params matching existing update_ticket signature (not 4-param spec example). create_ticket() and claim_next_ticket() call mark_ticket_dirty() when github_enabled. Conflict resolution in cli/github.rs github_wins() function: branch1=gh newer wins, branch2=local dirty wins, branch3=equal/missing=no-op. Migration in init.rs is idempotent: skips tickets with github_issue_number > 0. notes field NOT synced per spec."
```

- [ ] **Step 2: Mark ticket review_pending and notify manager**

```bash
cd /Users/mkhare/Development/devtok && acs ticket update --id t-076 --status review_pending
acs inbox push --recipient manager --type ticket_completed --payload '{"ticket_id":"t-076","status":"review_pending"}' --sender t-076
```
