# Design: GitHub Issues as Ticket Backend

**Date:** 2026-03-20
**Status:** Approved (v2 — post-review)

## Problem

ACS tickets live exclusively in `.acs/project.db`. There is no external visibility — you cannot see what workers are doing, comment on tickets, or manage the backlog from the GitHub UI.

## Goal

Replace the SQLite ticket store with **GitHub Issues as the authoritative ticket source**, keeping SQLite as a fast local cache. Everything else (KB, inbox, events, agent registry) stays in SQLite. **One ACS project per GitHub repo** — this is a stated constraint; two ACS projects must not share the same repo.

## Architecture

```
┌─────────────────────────────────────────────────────┐
│                  GitHub Issues                       │
│  Issues · Labels (status/domain/priority) · Milestones│
└────────────────────┬────────────────────────────────┘
                     │  gh api (via std::process::Command)
                     ▼
┌─────────────────────────────────────────────────────┐
│              src/github.rs  (GithubClient)          │
│  push_ticket() · pull_tickets() · sync_labels()     │
└────────────────────┬────────────────────────────────┘
                     ▼
┌──────────────────────────────────────────────────────┐
│  SQLite tickets cache  (github_* columns)            │
│  + agents · inbox · knowledge · events (unchanged)  │
└──────────────────────────────────────────────────────┘
```

**GitHub is authoritative on conflict.** `github_updated_at` (stored as a DB column) tracks the last GitHub-side timestamp; if a pull finds `gh_issue.updated_at > local.github_updated_at`, GitHub wins.

## Data Model Changes

### New `GithubTicketMeta` struct (`src/github.rs`)

Rather than bloating `Ticket`, sync metadata lives in a separate struct:

```rust
pub struct GithubTicketMeta {
    pub ticket_id: String,
    pub issue_number: u64,
    pub dirty: bool,
    pub synced_at: Option<String>,    // last time we pushed to GitHub
    pub updated_at: Option<String>,   // last GitHub-side updated_at
}
```

`pull_tickets()` returns `Vec<(Ticket, GithubTicketMeta)>`. `push_ticket()` returns `u64` (the issue number). The `Ticket` struct itself is **not changed**.

### New SQLite columns on `tickets`

```sql
ALTER TABLE tickets ADD COLUMN github_issue_number INTEGER;
ALTER TABLE tickets ADD COLUMN github_dirty         INTEGER NOT NULL DEFAULT 0;
ALTER TABLE tickets ADD COLUMN github_synced_at     TEXT;
ALTER TABLE tickets ADD COLUMN github_updated_at    TEXT;
```

### New `Db` methods

```rust
impl Db {
    pub fn set_github_meta(&self, ticket_id: &str, meta: &GithubTicketMeta) -> Result<()>
    pub fn get_github_meta(&self, ticket_id: &str) -> Result<Option<GithubTicketMeta>>
    pub fn list_dirty_tickets(&self) -> Result<Vec<(Ticket, GithubTicketMeta)>>
    pub fn mark_ticket_dirty(&self, ticket_id: &str) -> Result<()>
}
```

### Dirty flag propagation

`Db::mark_ticket_dirty()` is called explicitly after every write that should propagate to GitHub. Rather than modifying every call site, add a new `Db::update_ticket_and_mark_dirty()` wrapper used by the manager and worker paths:

```rust
pub fn update_ticket_and_mark_dirty(&self, id: &str, status: &str, notes: Option<&str>, assignee: Option<&str>) -> Result<()> {
    self.update_ticket(id, status, notes, assignee)?;
    if self.github_enabled { self.mark_ticket_dirty(id)?; }
    Ok(())
}
```

`github_enabled` is a `bool` field on `Db`, set at construction time from config. `create_ticket()` and `claim_next_ticket()` also call `mark_ticket_dirty()` after their SQL mutations.

## Config

New `GithubConfig` struct in `src/config.rs`:

```rust
#[derive(Deserialize, Clone, Default)]
pub struct GithubConfig {
    pub enabled: bool,
    pub repo: String,                    // "owner/repo" — auto-detected via detect_repo() if blank; if detection fails, github sync is disabled for the session with a one-time stderr warning
    pub sync_interval_seconds: u64,      // default: 60
    pub create_labels_on_init: bool,     // default: true
}
```

Added to `Config`:
```rust
pub struct Config {
    // existing fields ...
    pub github: GithubConfig,
}
```

`Config::to_toml()` gains a `[github]` serialization block. The round-trip test in `config.rs` is updated to include `github` fields.

## Ticket ↔ Issue Mapping

| ACS field       | GitHub Issues field                          |
|-----------------|----------------------------------------------|
| `id` (`t-NNN`)  | Label `acs-id:t-NNN` on the issue            |
| `title`         | Issue title                                  |
| `description`   | Issue body (up to separator `---`)           |
| `status`        | Label `status:*` + open/closed state         |
| `domain`        | Label `domain:*`                             |
| `priority`      | Label `priority:*`                           |
| `blocked_by`    | Issue body footer line `Blocked-by: t-NNN`   |
| `notes`         | **Not synced** (internal implementation detail — rate-limit strings, worker progress notes are not user-facing and would be noisy on GitHub) |

### Status → state + label

| ACS status            | GitHub state | Label                     |
|-----------------------|--------------|---------------------------|
| `pending`             | open         | `status:pending`          |
| `in_progress`         | open         | `status:in_progress`      |
| `review_pending`      | open         | `status:review_pending`   |
| `blocked`             | open         | `status:blocked`          |
| `awaiting_approval`   | open         | `status:awaiting_approval`|
| `completed`           | **closed**   | `status:completed`        |
| `rejected`            | **closed**   | `status:rejected`         |

## `src/github.rs` — GithubClient

```rust
pub struct GithubClient { repo: String }

impl GithubClient {
    pub fn new(repo: &str) -> Self
    pub fn detect_repo() -> Result<String>        // parse git remote origin (SSH + HTTPS)
    pub fn setup_labels(&self) -> Result<()>       // idempotent; skip existing labels
    pub fn push_ticket(&self, t: &Ticket) -> Result<u64>          // create issue, return number
    pub fn update_ticket(&self, number: u64, t: &Ticket) -> Result<String> // returns gh updated_at
    pub fn close_issue(&self, number: u64, label: &str) -> Result<String>
    pub fn pull_tickets(&self) -> Result<Vec<(Ticket, GithubTicketMeta)>>
}
```

All methods call `gh api` via `std::process::Command`. No new Rust dependencies added.

### Authentication (priority order)

1. `GITHUB_TOKEN` env var → passed as `GH_TOKEN=<token>` to `gh` subprocess
2. `gh auth token` → read from stdout
3. Error: `"Run gh auth login or set GITHUB_TOKEN"`

## New CLI Commands

### `acs github setup`
Creates all standard labels in the repo. Idempotent — skips labels that already exist. Added as `Commands::Github(GithubCommands)` subcommand group.

### `acs github sync`
Force full bidirectional sync. Pulls all issues from GitHub, upserts cache, pushes all dirty local tickets.

### `acs github status`
Shows: N dirty tickets, last sync time, `github_issue_number` coverage (N/M tickets have issue numbers), `gh` auth status.

### `acs init` — `--github` flag

Add `github: bool` to `Commands::Init` variant and `init::execute(spec, auto, github)` signature. When `--github` is passed: run `acs github setup`, then migrate existing tickets (see below).

## Migration (`acs init --github`)

```
for each ticket in db where github_issue_number IS NULL:
    push_ticket(ticket)  → get issue_number
    set_github_meta(ticket.id, {issue_number, dirty: false, ...})
    on error: log warning, continue (partial migration is safe — retry on next sync)
```

**Idempotency:** tickets with `github_issue_number IS NOT NULL` are skipped. Running twice is safe.
**Partial failure:** non-fatal; any ticket without an issue number will be pushed on the next background sync cycle.

## Background Sync Task

In `run.rs`, a new tokio task runs every `sync_interval_seconds`:

```rust
loop {
    sleep(Duration::from_secs(config.github.sync_interval_seconds)).await;
    let now = chrono::Utc::now().to_rfc3339();
    let dirty = { db.lock().unwrap().list_dirty_tickets()? };
    for (ticket, meta) in dirty {
        if meta.issue_number > 0 {
            let updated_at = github.update_ticket(meta.issue_number, &ticket)?;
            // Re-acquire lock per call (not held across await points)
            db.lock().unwrap().set_github_meta(&ticket.id, &GithubTicketMeta {
                dirty: false,
                synced_at: Some(now.clone()),   // wall-clock time of this push
                updated_at: Some(updated_at),   // GitHub-side timestamp
                ..meta
            })?;
        } else {
            let number = github.push_ticket(&ticket)?;
            db.lock().unwrap().set_github_meta(&ticket.id, &GithubTicketMeta {
                issue_number: number,
                dirty: false,
                synced_at: Some(now.clone()),
                ..meta
            })?;
        }
    }
}
```

Only spawned when `config.github.enabled`. Gated by `ACS_SKIP_GITHUB_SYNC=1` env var for tests.
The `db` lock is re-acquired per call, never held across `await` points.

## Conflict Resolution

On `pull_tickets()`, for each GitHub issue:
1. Look up local cache by `github_issue_number`
2. If `gh_issue.updated_at > local.github_updated_at`: update local cache (GitHub wins)
3. Else if `local.github_dirty = 1`: keep local (local change pending push)
4. Else: no-op

## What Does NOT Move to GitHub

| Stays in SQLite | Reason |
|---|---|
| `agents` | Ephemeral worker registry |
| `inbox` | High-frequency (~3s polling), would exhaust API rate limits |
| `knowledge` (KB) | Internal worker context |
| `events` | High-frequency audit log |
| `notes` | Internal implementation strings, not user-facing |

## Backwards Compatibility

`github.enabled = false` by default. All existing behaviour unchanged. Enable with `acs init --github` or `[github] enabled = true` in `config.toml`.

## Testing

- Unit: `detect_repo()` parses SSH (`git@github.com:owner/repo.git`) and HTTPS (`https://github.com/owner/repo`) remote URLs
- Unit: label creation is idempotent (mock `gh api` returns 422 for existing label, handler skips)
- Unit: status→label round-trip for all 7 statuses
- Unit: `mark_ticket_dirty` → `list_dirty_tickets` returns expected rows
- Unit: conflict resolution: `updated_at` comparison covers all 3 branches
- Unit: migration is idempotent (ticket with existing `github_issue_number` not re-pushed)
- Integration: `ACS_GITHUB_MOCK=1` reads fixture JSON from `.acs/test-fixtures/github/` — follows existing `ACS_SKIP_*` env var pattern
- Existing tests: all pass unchanged (github disabled by default, `Ticket` struct unchanged)
