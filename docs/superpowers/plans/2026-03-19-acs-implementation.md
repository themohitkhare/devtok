# ACS (Auto Consulting Service) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build a Rust CLI tool (`acs`) that manages autonomous AI development teams via Claude Code CLI subprocesses, with SQLite storage and git worktree isolation.

**Architecture:** Single Rust binary with modules: db (SQLite), spawner (process management + worktrees), prompts (persona system prompts), manager (ticket assignment loop), worker (execution loop), cli (clap subcommands). Agents communicate with the brain via `acs` CLI subcommands through Bash.

**Tech Stack:** Rust 2024 edition, clap (CLI), rusqlite (SQLite), serde/serde_json (serialization), tokio (async runtime for parallel workers), chrono (timestamps)

---

## File Structure

```
src/
├── main.rs              # Entry point — clap CLI dispatch
├── db.rs                # SQLite schema, migrations, typed queries
├── models.rs            # Ticket, Agent, InboxMessage, Event structs
├── spawner.rs           # Git worktree + Claude Code process management
├── prompts.rs           # System prompt generators for each persona
├── manager.rs           # Manager loop — claim tickets, assign workers
├── worker.rs            # Worker loop — poll inbox, execute tickets
├── config.rs            # Config file parsing (TOML)
└── cli/
    ├── mod.rs           # Clap command definitions
    ├── init.rs          # acs init command
    ├── run.rs           # acs run command
    ├── status.rs        # acs status command
    ├── log.rs           # acs log command
    ├── ticket.rs        # acs ticket subcommands
    ├── kb.rs            # acs kb subcommands
    └── inbox.rs         # acs inbox subcommands

tests/
├── db_test.rs           # DB integration tests
├── spawner_test.rs      # Spawner tests (mock git/claude)
├── manager_test.rs      # Manager logic tests
└── worker_test.rs       # Worker logic tests
```

---

### Task 1: Project Setup + Dependencies

**Files:**
- Modify: `Cargo.toml`
- Create: `src/main.rs`
- Create: `src/models.rs`

- [ ] **Step 1: Set up Cargo.toml with all dependencies**

```toml
[package]
name = "acs"
version = "0.1.0"
edition = "2024"

[dependencies]
clap = { version = "4", features = ["derive"] }
rusqlite = { version = "0.35", features = ["bundled"] }
serde = { version = "1", features = ["derive"] }
serde_json = "1"
toml = "0.8"
chrono = { version = "0.4", features = ["serde"] }
tokio = { version = "1", features = ["full"] }
rand = "0.9"
anyhow = "1"

[dev-dependencies]
tempfile = "3"
```

- [ ] **Step 2: Create models.rs with core data types**

```rust
// src/models.rs
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Ticket {
    pub id: String,
    pub title: String,
    pub description: String,
    pub domain: String,
    pub priority: i32,
    pub status: String,
    pub assignee: Option<String>,
    pub blocked_by: Option<String>,
    pub notes: String,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Agent {
    pub id: String,
    pub role: String,
    pub persona: String,
    pub status: String,
    pub current_ticket: Option<String>,
    pub pid: Option<u32>,
    pub last_heartbeat: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InboxMessage {
    pub id: i64,
    pub recipient: String,
    pub msg_type: String,
    pub payload: String,
    pub sender: String,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KnowledgeEntry {
    pub domain: String,
    pub key: String,
    pub value: String,
    pub version: i32,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Event {
    pub id: i64,
    pub timestamp: String,
    pub agent: Option<String>,
    pub event_type: String,
    pub detail: String,
    pub tokens_used: Option<i64>,
}
```

- [ ] **Step 3: Create lib.rs (for tests to import) and main.rs**

```rust
// src/lib.rs
pub mod models;

// Will add more modules as they're created:
// pub mod db;
// pub mod config;
// pub mod prompts;
// pub mod spawner;
// pub mod manager;
// pub mod worker;
// pub mod cli;
```

```rust
// src/main.rs
fn main() {
    println!("acs v0.1.0");
}
```

- [ ] **Step 4: Verify it compiles**

Run: `cargo build`
Expected: Compiles successfully

- [ ] **Step 5: Commit**

```bash
git add Cargo.toml src/
git commit -m "feat: project setup with dependencies and core data models"
```

---

### Task 2: SQLite Database Module

**Files:**
- Create: `src/db.rs`
- Create: `tests/db_test.rs`
- Modify: `src/main.rs` (add `mod db`)

- [ ] **Step 1: Write db.rs with schema creation and all typed queries**

```rust
// src/db.rs
use anyhow::{Context, Result};
use chrono::Utc;
use rusqlite::{params, Connection};
use std::path::Path;

use crate::models::*;

pub struct Db {
    conn: Connection,
}

impl Db {
    pub fn open(path: &Path) -> Result<Self> {
        let conn = Connection::open(path)?;
        conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON;")?;
        let db = Db { conn };
        db.migrate()?;
        Ok(db)
    }

    pub fn open_memory() -> Result<Self> {
        let conn = Connection::open_in_memory()?;
        let db = Db { conn };
        db.migrate()?;
        Ok(db)
    }

    fn migrate(&self) -> Result<()> {
        self.conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS tickets (
                id TEXT PRIMARY KEY,
                title TEXT NOT NULL,
                description TEXT NOT NULL,
                domain TEXT NOT NULL,
                priority INTEGER NOT NULL,
                status TEXT NOT NULL DEFAULT 'pending',
                assignee TEXT,
                blocked_by TEXT,
                notes TEXT DEFAULT '',
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL
            );
            CREATE TABLE IF NOT EXISTS agents (
                id TEXT PRIMARY KEY,
                role TEXT NOT NULL,
                persona TEXT NOT NULL DEFAULT 'general',
                status TEXT NOT NULL DEFAULT 'idle',
                current_ticket TEXT,
                pid INTEGER,
                last_heartbeat TEXT
            );
            CREATE TABLE IF NOT EXISTS inbox (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                recipient TEXT NOT NULL,
                msg_type TEXT NOT NULL,
                payload TEXT NOT NULL,
                sender TEXT NOT NULL,
                created_at TEXT NOT NULL,
                read INTEGER NOT NULL DEFAULT 0
            );
            CREATE INDEX IF NOT EXISTS idx_inbox_recipient_unread ON inbox(recipient, read);
            CREATE TABLE IF NOT EXISTS knowledge (
                domain TEXT NOT NULL,
                key TEXT NOT NULL,
                value TEXT NOT NULL,
                version INTEGER NOT NULL DEFAULT 1,
                updated_at TEXT NOT NULL,
                PRIMARY KEY (domain, key)
            );
            CREATE TABLE IF NOT EXISTS events (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                timestamp TEXT NOT NULL,
                agent TEXT,
                event_type TEXT NOT NULL,
                detail TEXT NOT NULL,
                tokens_used INTEGER DEFAULT 0
            );
            CREATE TABLE IF NOT EXISTS counters (
                name TEXT PRIMARY KEY,
                value INTEGER NOT NULL DEFAULT 0
            );
            INSERT OR IGNORE INTO counters (name, value) VALUES ('ticket_id', 0);"
        )?;
        Ok(())
    }

    // --- Tickets ---

    pub fn create_ticket(&self, title: &str, desc: &str, domain: &str, priority: i32) -> Result<String> {
        let now = Utc::now().to_rfc3339();
        let id: String = self.conn.query_row(
            "UPDATE counters SET value = value + 1 WHERE name = 'ticket_id' RETURNING value",
            [],
            |row| {
                let n: i64 = row.get(0)?;
                Ok(format!("t-{:03}", n))
            },
        )?;
        self.conn.execute(
            "INSERT INTO tickets (id, title, description, domain, priority, status, notes, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, 'pending', '', ?6, ?6)",
            params![id, title, desc, domain, priority, now],
        )?;
        Ok(id)
    }

    pub fn get_ticket(&self, id: &str) -> Result<Option<Ticket>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, title, description, domain, priority, status, assignee, blocked_by, notes, created_at, updated_at
             FROM tickets WHERE id = ?1"
        )?;
        let ticket = stmt.query_row(params![id], |row| {
            Ok(Ticket {
                id: row.get(0)?, title: row.get(1)?, description: row.get(2)?,
                domain: row.get(3)?, priority: row.get(4)?, status: row.get(5)?,
                assignee: row.get(6)?, blocked_by: row.get(7)?, notes: row.get(8)?,
                created_at: row.get(9)?, updated_at: row.get(10)?,
            })
        }).optional()?;
        Ok(ticket)
    }

    pub fn list_tickets(&self, status: Option<&str>) -> Result<Vec<Ticket>> {
        let sql = match status {
            Some(_) => "SELECT id, title, description, domain, priority, status, assignee, blocked_by, notes, created_at, updated_at FROM tickets WHERE status = ?1 ORDER BY priority, created_at",
            None => "SELECT id, title, description, domain, priority, status, assignee, blocked_by, notes, created_at, updated_at FROM tickets ORDER BY priority, created_at",
        };
        let mut stmt = self.conn.prepare(sql)?;
        let rows = if let Some(s) = status {
            stmt.query_map(params![s], Self::row_to_ticket)?
        } else {
            stmt.query_map([], Self::row_to_ticket)?
        };
        rows.map(|r| r.map_err(Into::into)).collect()
    }

    pub fn update_ticket(&self, id: &str, status: &str, notes: Option<&str>, blocked_by: Option<&str>) -> Result<()> {
        let now = Utc::now().to_rfc3339();
        self.conn.execute(
            "UPDATE tickets SET status = ?2, notes = COALESCE(?3, notes), blocked_by = ?4, updated_at = ?5 WHERE id = ?1",
            params![id, status, notes, blocked_by, now],
        )?;
        Ok(())
    }

    pub fn claim_next_ticket(&self, agent_id: &str) -> Result<Option<Ticket>> {
        let now = Utc::now().to_rfc3339();
        let tx = self.conn.unchecked_transaction()?;
        // Update and return in one step using RETURNING
        let ticket: Option<Ticket> = tx.query_row(
            "UPDATE tickets SET status = 'in_progress', assignee = ?1, updated_at = ?2
             WHERE id = (SELECT id FROM tickets WHERE status = 'pending' AND assignee IS NULL ORDER BY priority, created_at LIMIT 1)
             RETURNING id, title, description, domain, priority, status, assignee, blocked_by, notes, created_at, updated_at",
            params![agent_id, now],
            Self::row_to_ticket,
        ).optional()?;
        tx.commit()?;
        Ok(ticket)
    }

    pub fn count_by_status(&self) -> Result<Vec<(String, i64)>> {
        let mut stmt = self.conn.prepare("SELECT status, COUNT(*) FROM tickets GROUP BY status")?;
        let rows = stmt.query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?;
        rows.map(|r| r.map_err(Into::into)).collect()
    }

    fn row_to_ticket(row: &rusqlite::Row) -> rusqlite::Result<Ticket> {
        Ok(Ticket {
            id: row.get(0)?, title: row.get(1)?, description: row.get(2)?,
            domain: row.get(3)?, priority: row.get(4)?, status: row.get(5)?,
            assignee: row.get(6)?, blocked_by: row.get(7)?, notes: row.get(8)?,
            created_at: row.get(9)?, updated_at: row.get(10)?,
        })
    }

    // --- Inbox ---

    pub fn push_inbox(&self, recipient: &str, msg_type: &str, payload: &str, sender: &str) -> Result<()> {
        let now = Utc::now().to_rfc3339();
        self.conn.execute(
            "INSERT INTO inbox (recipient, msg_type, payload, sender, created_at) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![recipient, msg_type, payload, sender, now],
        )?;
        Ok(())
    }

    pub fn pop_inbox(&self, agent_id: &str) -> Result<Option<InboxMessage>> {
        let tx = self.conn.unchecked_transaction()?;
        let msg: Option<InboxMessage> = tx.query_row(
            "SELECT id, recipient, msg_type, payload, sender, created_at FROM inbox WHERE recipient = ?1 AND read = 0 ORDER BY id LIMIT 1",
            params![agent_id],
            |row| Ok(InboxMessage {
                id: row.get(0)?, recipient: row.get(1)?, msg_type: row.get(2)?,
                payload: row.get(3)?, sender: row.get(4)?, created_at: row.get(5)?,
            }),
        ).optional()?;
        if let Some(ref m) = msg {
            tx.execute("UPDATE inbox SET read = 1 WHERE id = ?1", params![m.id])?;
        }
        tx.commit()?;
        Ok(msg)
    }

    // --- Knowledge ---

    pub fn read_knowledge(&self, domain: &str, key: &str) -> Result<Option<KnowledgeEntry>> {
        self.conn.query_row(
            "SELECT domain, key, value, version, updated_at FROM knowledge WHERE domain = ?1 AND key = ?2",
            params![domain, key],
            |row| Ok(KnowledgeEntry {
                domain: row.get(0)?, key: row.get(1)?, value: row.get(2)?,
                version: row.get(3)?, updated_at: row.get(4)?,
            }),
        ).optional().map_err(Into::into)
    }

    pub fn write_knowledge(&self, domain: &str, key: &str, value: &str) -> Result<()> {
        let now = Utc::now().to_rfc3339();
        self.conn.execute(
            "INSERT INTO knowledge (domain, key, value, version, updated_at) VALUES (?1, ?2, ?3, 1, ?4)
             ON CONFLICT(domain, key) DO UPDATE SET value = ?3, version = version + 1, updated_at = ?4",
            params![domain, key, value, now],
        )?;
        Ok(())
    }

    // --- Agents ---

    pub fn register_agent(&self, id: &str, role: &str, persona: &str) -> Result<()> {
        self.conn.execute(
            "INSERT OR REPLACE INTO agents (id, role, persona, status) VALUES (?1, ?2, ?3, 'idle')",
            params![id, role, persona],
        )?;
        Ok(())
    }

    pub fn update_agent(&self, id: &str, status: &str, ticket: Option<&str>, pid: Option<u32>) -> Result<()> {
        let now = Utc::now().to_rfc3339();
        self.conn.execute(
            "UPDATE agents SET status = ?2, current_ticket = ?3, pid = ?4, last_heartbeat = ?5 WHERE id = ?1",
            params![id, status, ticket, pid, now],
        )?;
        Ok(())
    }

    pub fn list_agents(&self) -> Result<Vec<Agent>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, role, persona, status, current_ticket, pid, last_heartbeat FROM agents"
        )?;
        let rows = stmt.query_map([], |row| Ok(Agent {
            id: row.get(0)?, role: row.get(1)?, persona: row.get(2)?,
            status: row.get(3)?, current_ticket: row.get(4)?,
            pid: row.get(5)?, last_heartbeat: row.get(6)?,
        }))?;
        rows.map(|r| r.map_err(Into::into)).collect()
    }

    pub fn deregister_agent(&self, id: &str) -> Result<()> {
        self.conn.execute("DELETE FROM agents WHERE id = ?1", params![id])?;
        Ok(())
    }

    // --- Events ---

    pub fn log_event(&self, agent: Option<&str>, event_type: &str, detail: &str, tokens: Option<i64>) -> Result<()> {
        let now = Utc::now().to_rfc3339();
        self.conn.execute(
            "INSERT INTO events (timestamp, agent, event_type, detail, tokens_used) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![now, agent, event_type, detail, tokens.unwrap_or(0)],
        )?;
        Ok(())
    }

    pub fn recent_events(&self, limit: usize) -> Result<Vec<Event>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, timestamp, agent, event_type, detail, tokens_used FROM events ORDER BY id DESC LIMIT ?1"
        )?;
        let rows = stmt.query_map(params![limit as i64], |row| Ok(Event {
            id: row.get(0)?, timestamp: row.get(1)?, agent: row.get(2)?,
            event_type: row.get(3)?, detail: row.get(4)?, tokens_used: row.get(5)?,
        }))?;
        rows.map(|r| r.map_err(Into::into)).collect()
    }
}

// Required for rusqlite optional queries
use rusqlite::OptionalExtension;
```

- [ ] **Step 2: Write tests**

```rust
// tests/db_test.rs
use acs::db::Db;

#[test]
fn test_create_and_get_ticket() {
    let db = Db::open_memory().unwrap();
    let id = db.create_ticket("Build auth", "Add login", "backend", 1).unwrap();
    assert_eq!(id, "t-001");
    let ticket = db.get_ticket(&id).unwrap().unwrap();
    assert_eq!(ticket.title, "Build auth");
    assert_eq!(ticket.status, "pending");
}

#[test]
fn test_sequential_ticket_ids() {
    let db = Db::open_memory().unwrap();
    let id1 = db.create_ticket("First", "Desc", "backend", 1).unwrap();
    let id2 = db.create_ticket("Second", "Desc", "frontend", 2).unwrap();
    assert_eq!(id1, "t-001");
    assert_eq!(id2, "t-002");
}

#[test]
fn test_claim_next_ticket() {
    let db = Db::open_memory().unwrap();
    db.create_ticket("High priority", "Desc", "backend", 1).unwrap();
    db.create_ticket("Low priority", "Desc", "backend", 5).unwrap();
    let claimed = db.claim_next_ticket("w-0").unwrap().unwrap();
    assert_eq!(claimed.title, "High priority");
    assert_eq!(claimed.status, "in_progress");
    assert_eq!(claimed.assignee.as_deref(), Some("w-0"));
}

#[test]
fn test_claim_empty_returns_none() {
    let db = Db::open_memory().unwrap();
    let claimed = db.claim_next_ticket("w-0").unwrap();
    assert!(claimed.is_none());
}

#[test]
fn test_inbox_push_pop() {
    let db = Db::open_memory().unwrap();
    db.push_inbox("w-0", "ticket_assignment", r#"{"ticket_id":"t-001"}"#, "mgr").unwrap();
    let msg = db.pop_inbox("w-0").unwrap().unwrap();
    assert_eq!(msg.msg_type, "ticket_assignment");
    assert_eq!(msg.sender, "mgr");
    // Second pop returns None (already read)
    let msg2 = db.pop_inbox("w-0").unwrap();
    assert!(msg2.is_none());
}

#[test]
fn test_knowledge_write_read() {
    let db = Db::open_memory().unwrap();
    db.write_knowledge("backend", "stack", "Rust, Axum").unwrap();
    let entry = db.read_knowledge("backend", "stack").unwrap().unwrap();
    assert_eq!(entry.value, "Rust, Axum");
    assert_eq!(entry.version, 1);
    // Overwrite bumps version
    db.write_knowledge("backend", "stack", "Rust, Actix").unwrap();
    let entry2 = db.read_knowledge("backend", "stack").unwrap().unwrap();
    assert_eq!(entry2.value, "Rust, Actix");
    assert_eq!(entry2.version, 2);
}

#[test]
fn test_agent_lifecycle() {
    let db = Db::open_memory().unwrap();
    db.register_agent("w-0", "worker", "backend-dev").unwrap();
    let agents = db.list_agents().unwrap();
    assert_eq!(agents.len(), 1);
    assert_eq!(agents[0].status, "idle");

    db.update_agent("w-0", "working", Some("t-001"), Some(12345)).unwrap();
    let agents = db.list_agents().unwrap();
    assert_eq!(agents[0].status, "working");

    db.deregister_agent("w-0").unwrap();
    assert!(db.list_agents().unwrap().is_empty());
}

#[test]
fn test_events() {
    let db = Db::open_memory().unwrap();
    db.log_event(Some("w-0"), "ticket_completed", "Completed t-001", Some(1500)).unwrap();
    let events = db.recent_events(10).unwrap();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].tokens_used, Some(1500));
}
```

- [ ] **Step 3: Update main.rs to export db module**

```rust
// src/main.rs
pub mod db;
pub mod models;

fn main() {
    println!("acs v0.1.0");
}
```

- [ ] **Step 4: Run tests**

Run: `cargo test`
Expected: 8 tests pass

- [ ] **Step 5: Commit**

```bash
git add src/db.rs tests/db_test.rs src/main.rs
git commit -m "feat: SQLite database module with tickets, inbox, knowledge base, agents, events"
```

---

### Task 3: Config Module

**Files:**
- Create: `src/config.rs`
- Modify: `src/main.rs`

- [ ] **Step 1: Write config.rs**

```rust
// src/config.rs
use anyhow::Result;
use serde::Deserialize;
use std::collections::HashMap;
use std::path::Path;

#[derive(Debug, Deserialize)]
pub struct Config {
    pub project: ProjectConfig,
    #[serde(default)]
    pub manager: ManagerConfig,
    #[serde(default)]
    pub personas: PersonaConfig,
    #[serde(default)]
    pub agents: AgentConfig,
}

#[derive(Debug, Deserialize)]
pub struct ProjectConfig {
    pub name: String,
    #[serde(default = "default_workers")]
    pub default_workers: usize,
}

#[derive(Debug, Deserialize)]
pub struct ManagerConfig {
    #[serde(default = "default_cycle")]
    pub cycle_seconds: u64,
    #[serde(default = "default_timeout")]
    pub worker_timeout_seconds: u64,
    #[serde(default = "default_poll")]
    pub worker_poll_seconds: u64,
}

#[derive(Debug, Deserialize)]
pub struct PersonaConfig {
    #[serde(default = "default_persona_mapping")]
    pub mapping: HashMap<String, String>,
}

#[derive(Debug, Deserialize)]
pub struct AgentConfig {
    #[serde(default = "default_tool_path")]
    pub tool_path: String,
    #[serde(default = "default_claude_path")]
    pub claude_path: String,
}

fn default_workers() -> usize { 2 }
fn default_cycle() -> u64 { 15 }
fn default_timeout() -> u64 { 300 }
fn default_poll() -> u64 { 3 }
fn default_tool_path() -> String { "acs".into() }
fn default_claude_path() -> String { "claude".into() }

fn default_persona_mapping() -> HashMap<String, String> {
    let mut m = HashMap::new();
    m.insert("frontend".into(), "frontend-dev".into());
    m.insert("backend".into(), "backend-dev".into());
    m.insert("devops".into(), "devops".into());
    m.insert("qa".into(), "qa".into());
    m.insert("infra".into(), "devops".into());
    m.insert("core".into(), "tech-lead".into());
    m.insert("general".into(), "backend-dev".into());
    m
}

impl Default for ManagerConfig {
    fn default() -> Self {
        Self { cycle_seconds: default_cycle(), worker_timeout_seconds: default_timeout(), worker_poll_seconds: default_poll() }
    }
}

impl Default for PersonaConfig {
    fn default() -> Self {
        Self { mapping: default_persona_mapping() }
    }
}

impl Default for AgentConfig {
    fn default() -> Self {
        Self { tool_path: default_tool_path(), claude_path: default_claude_path() }
    }
}

impl Config {
    pub fn load(path: &Path) -> Result<Self> {
        let content = std::fs::read_to_string(path)?;
        Ok(toml::from_str(&content)?)
    }

    pub fn default_for(project_name: &str) -> Self {
        Config {
            project: ProjectConfig { name: project_name.into(), default_workers: 2 },
            manager: ManagerConfig::default(),
            personas: PersonaConfig::default(),
            agents: AgentConfig::default(),
        }
    }

    pub fn persona_for_domain(&self, domain: &str) -> &str {
        self.personas.mapping.get(domain).map(|s| s.as_str()).unwrap_or("backend-dev")
    }

    pub fn to_toml(&self) -> String {
        format!(
            r#"[project]
name = "{}"
default_workers = {}

[manager]
cycle_seconds = {}
worker_timeout_seconds = {}
worker_poll_seconds = {}

[agents]
tool_path = "{}"
claude_path = "{}"
"#,
            self.project.name, self.project.default_workers,
            self.manager.cycle_seconds, self.manager.worker_timeout_seconds, self.manager.worker_poll_seconds,
            self.agents.tool_path, self.agents.claude_path,
        )
    }
}
```

- [ ] **Step 2: Add mod, verify**

Add `pub mod config;` to `main.rs`. Run: `cargo build`

- [ ] **Step 3: Commit**

```bash
git add src/config.rs src/main.rs
git commit -m "feat: config module with TOML parsing and persona mapping"
```

---

### Task 4: System Prompts

**Files:**
- Create: `src/prompts.rs`
- Modify: `src/main.rs`

- [ ] **Step 1: Write prompts.rs with all persona generators**

The prompts must tell agents to use `acs` CLI commands via Bash. Include exact command examples. Each persona gets behavioral instructions matching their role. The `tool_path` parameter is the resolved path to the `acs` binary.

Key prompts: `bootstrap_prompt(repo_path, spec_text, tool_path)`, `worker_prompt(ticket, persona, tool_path)`.

The manager is pure Rust — no prompt needed.

- [ ] **Step 2: Verify compiles**

Run: `cargo build`

- [ ] **Step 3: Commit**

```bash
git add src/prompts.rs src/main.rs
git commit -m "feat: system prompts for bootstrap and worker personas"
```

---

### Task 5: Spawner (Git Worktrees + Claude Code)

**Files:**
- Create: `src/spawner.rs`
- Modify: `src/main.rs`

- [ ] **Step 1: Write spawner.rs**

Implements:
- `create_worktree(worker_id, ticket_id)` — runs `git worktree add .acs/worktrees/{worker_id} -b acs/{ticket_id}-{rand4}`
- `remove_worktree(worker_id)` — runs `git worktree remove .acs/worktrees/{worker_id} --force`
- `spawn_claude(worker_id, worktree_path, prompt, system_prompt, log_path)` — spawns `claude -p ... --dangerously-skip-permissions --output-format json` with cwd=worktree, stdout/stderr redirected to log file. Returns `Child`.
- `kill_process(pid)` — sends SIGTERM, waits 5s, then SIGKILL.

Uses `std::process::Command` for git commands (synchronous, quick).
Uses `tokio::process::Command` for Claude Code (async, long-running).

- [ ] **Step 2: Verify compiles**

Run: `cargo build`

- [ ] **Step 3: Commit**

```bash
git add src/spawner.rs src/main.rs
git commit -m "feat: spawner module — git worktrees + Claude Code process management"
```

---

### Task 6: Manager Module

**Files:**
- Create: `src/manager.rs`
- Modify: `src/main.rs`

- [ ] **Step 1: Write manager.rs**

Pure Rust logic, no LLM. The manager loop:
1. `claim_and_assign(db, config)` — for each idle worker, claim next ticket, look up persona, push assignment to worker inbox
2. `process_completions(db)` — read manager inbox, mark tickets completed
3. `unblock_tickets(db)` — if a blocked ticket's `blocked_by` is now completed, reset to pending
4. `run_loop(db, config, shutdown_signal)` — async loop with sleep interval

```rust
pub async fn run_loop(db: Arc<Mutex<Db>>, config: &Config, shutdown: tokio::sync::watch::Receiver<bool>)
```

- [ ] **Step 2: Verify compiles**

Run: `cargo build`

- [ ] **Step 3: Commit**

```bash
git add src/manager.rs src/main.rs
git commit -m "feat: manager module — ticket assignment and completion loop"
```

---

### Task 7: Worker Module

**Files:**
- Create: `src/worker.rs`
- Modify: `src/main.rs`

- [ ] **Step 1: Write worker.rs**

Worker loop (runs as a tokio task per worker):
1. Poll inbox every `worker_poll_seconds`
2. On assignment: create worktree, spawn Claude Code, wait with timeout
3. On completion: parse JSON output for tokens, push completion to mgr inbox, clean up worktree
4. On timeout: kill process, set ticket to blocked, clean up
5. On crash: set ticket to pending (re-enqueue), clean up

```rust
pub async fn worker_loop(
    worker_id: String,
    db: Arc<Mutex<Db>>,
    config: Config,
    project_dir: PathBuf,
    shutdown: tokio::sync::watch::Receiver<bool>,
)
```

- [ ] **Step 2: Verify compiles**

Run: `cargo build`

- [ ] **Step 3: Commit**

```bash
git add src/worker.rs src/main.rs
git commit -m "feat: worker module — inbox polling, worktree execution, timeout handling"
```

---

### Task 8: CLI — Core Subcommands (ticket, kb, inbox)

**Files:**
- Create: `src/cli/mod.rs`
- Create: `src/cli/ticket.rs`
- Create: `src/cli/kb.rs`
- Create: `src/cli/inbox.rs`
- Modify: `src/main.rs`

- [ ] **Step 1: Write CLI module with clap derive macros**

These are the subcommands that agents call via Bash. Each opens `.acs/project.db`, performs the operation, prints JSON result to stdout.

```rust
// src/cli/mod.rs
pub mod ticket;
pub mod kb;
pub mod inbox;

use clap::Subcommand;

#[derive(Subcommand)]
pub enum Commands {
    Init { ... },
    Run { ... },
    Status,
    Log { ... },
    #[command(subcommand)]
    Ticket(ticket::TicketCommands),
    #[command(subcommand)]
    Kb(kb::KbCommands),
    #[command(subcommand)]
    Inbox(inbox::InboxCommands),
}
```

Each subcommand: open db → execute → print JSON → exit.

- [ ] **Step 2: Verify compiles**

Run: `cargo build`

- [ ] **Step 3: Test manually**

```bash
mkdir -p /tmp/test-acs/.acs
cargo run -- ticket create --title "Test" --description "A test ticket" --domain backend --priority 1
cargo run -- ticket list
cargo run -- kb write --domain backend --key stack --value "Rust"
cargo run -- kb read --domain backend --key stack
```

- [ ] **Step 4: Commit**

```bash
git add src/cli/ src/main.rs
git commit -m "feat: CLI subcommands — ticket, kb, inbox operations"
```

---

### Task 9: CLI — init Command

**Files:**
- Create: `src/cli/init.rs`
- Modify: `src/cli/mod.rs`

- [ ] **Step 1: Write init command**

1. Create `.acs/` directory
2. Create `.acs/config.toml` with defaults
3. Create `.acs/project.db` (migrations run automatically)
4. Create `.acs/logs/` directory
5. If `--spec`, read spec file
6. If `--auto` or `--spec`, spawn Claude Code bootstrap agent
7. Wait for completion, print summary

- [ ] **Step 2: Test manually**

```bash
cd /tmp/test-repo && git init && cargo run --manifest-path /path/to/acs/Cargo.toml -- init --auto
```

- [ ] **Step 3: Commit**

```bash
git add src/cli/init.rs src/cli/mod.rs
git commit -m "feat: acs init — bootstrap with spec file or auto-analyze"
```

---

### Task 10: CLI — run, status, log Commands

**Files:**
- Create: `src/cli/run.rs`
- Create: `src/cli/status.rs`
- Create: `src/cli/log.rs`
- Modify: `src/cli/mod.rs`

- [ ] **Step 1: Write run command**

1. Open db, load config
2. Register workers
3. Spawn manager task + N worker tasks (tokio)
4. Handle SIGINT for graceful shutdown
5. On shutdown: kill all Claude processes, remove worktrees, deregister agents

- [ ] **Step 2: Write status command**

Query db: ticket counts by status, agent list with current state, total tokens used.

- [ ] **Step 3: Write log command**

Query events table. `--follow` polls every second. `--limit N` caps output.

- [ ] **Step 4: Verify compiles and run**

Run: `cargo build && cargo run -- status`

- [ ] **Step 5: Commit**

```bash
git add src/cli/run.rs src/cli/status.rs src/cli/log.rs src/cli/mod.rs
git commit -m "feat: acs run, status, log commands — full execution pipeline"
```

---

### Task 11: Integration Test — Self-Bootstrap

**Files:**
- Create: `tests/integration_test.rs`

- [ ] **Step 1: Write integration test**

```rust
// tests/integration_test.rs
use std::process::Command;

#[test]
fn test_acs_help() {
    let output = Command::new("cargo").args(["run", "--", "--help"]).output().unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("init"));
    assert!(stdout.contains("run"));
    assert!(stdout.contains("status"));
}

#[test]
fn test_ticket_roundtrip() {
    // Create temp dir with .acs
    let dir = tempfile::tempdir().unwrap();
    let acs_dir = dir.path().join(".acs");
    std::fs::create_dir_all(&acs_dir).unwrap();

    let acs = env!("CARGO_BIN_EXE_acs");

    // Create ticket
    let output = Command::new(acs)
        .args(["ticket", "create", "--title", "Test ticket", "--description", "A test", "--domain", "backend", "--priority", "1"])
        .current_dir(dir.path())
        .output().unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("t-001"));

    // List tickets
    let output = Command::new(acs)
        .args(["ticket", "list"])
        .current_dir(dir.path())
        .output().unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Test ticket"));
}
```

- [ ] **Step 2: Run integration tests**

Run: `cargo test --test integration_test`

- [ ] **Step 3: Commit**

```bash
git add tests/integration_test.rs
git commit -m "test: integration tests — CLI roundtrip and help output"
```

---

### Task 12: Self-Bootstrap — Point ACS at Itself

**Files:** None — this is a manual verification step.

- [ ] **Step 1: Install ACS**

```bash
cargo install --path .
```

- [ ] **Step 2: Bootstrap ACS on its own repo**

```bash
acs init --auto
```

- [ ] **Step 3: Verify tickets created**

```bash
acs ticket list
acs status
```

- [ ] **Step 4: Run the team**

```bash
acs run --workers 3
```

- [ ] **Step 5: Monitor progress**

```bash
acs log --follow
```
