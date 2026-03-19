use anyhow::Result;
use chrono::Utc;
use rusqlite::{params, Connection};
use std::path::Path;

use crate::models::*;

// Required for rusqlite optional queries
use rusqlite::OptionalExtension;

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
