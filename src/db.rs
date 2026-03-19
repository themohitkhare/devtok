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
                tokens_used INTEGER DEFAULT 0,
                input_tokens INTEGER DEFAULT 0,
                output_tokens INTEGER DEFAULT 0,
                ticket_id TEXT,
                model TEXT
            );
            CREATE TABLE IF NOT EXISTS counters (
                name TEXT PRIMARY KEY,
                value INTEGER NOT NULL DEFAULT 0
            );
            INSERT OR IGNORE INTO counters (name, value) VALUES ('ticket_id', 0);
            CREATE TABLE IF NOT EXISTS ticket_keywords (
                ticket_id TEXT NOT NULL,
                keyword TEXT NOT NULL,
                PRIMARY KEY (ticket_id, keyword),
                FOREIGN KEY (ticket_id) REFERENCES tickets(id) ON DELETE CASCADE
            );
            CREATE INDEX IF NOT EXISTS idx_ticket_keywords_keyword ON ticket_keywords(keyword);
            CREATE TABLE IF NOT EXISTS milestones (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                name TEXT NOT NULL,
                goal TEXT NOT NULL,
                status TEXT NOT NULL DEFAULT 'pending',
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL
            );
            CREATE TABLE IF NOT EXISTS milestone_tickets (
                milestone_id INTEGER NOT NULL,
                ticket_id TEXT NOT NULL,
                PRIMARY KEY (milestone_id, ticket_id),
                FOREIGN KEY (milestone_id) REFERENCES milestones(id) ON DELETE CASCADE,
                FOREIGN KEY (ticket_id) REFERENCES tickets(id) ON DELETE CASCADE
            );
            CREATE INDEX IF NOT EXISTS idx_milestone_tickets_milestone ON milestone_tickets(milestone_id);"
        )?;

        // Additive migrations: add new columns to existing events tables.
        // SQLite returns an error when the column already exists; we ignore it.
        let _ = self.conn.execute_batch(
            "ALTER TABLE events ADD COLUMN input_tokens INTEGER DEFAULT 0;
             ALTER TABLE events ADD COLUMN output_tokens INTEGER DEFAULT 0;
             ALTER TABLE events ADD COLUMN ticket_id TEXT;
             ALTER TABLE events ADD COLUMN model TEXT;"
        );

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

    /// Update a ticket's status and optional fields.
    ///
    /// `assignee` uses `Option<Option<&str>>`:
    /// - `None` → leave assignee unchanged
    /// - `Some(None)` → clear assignee (set to NULL)
    /// - `Some(Some(val))` → set assignee to `val`
    pub fn update_ticket(
        &self,
        id: &str,
        status: &str,
        notes: Option<&str>,
        blocked_by: Option<&str>,
        assignee: Option<Option<&str>>,
    ) -> Result<()> {
        let now = Utc::now().to_rfc3339();
        match assignee {
            Some(new_assignee) => {
                self.conn.execute(
                    "UPDATE tickets SET status = ?2, notes = COALESCE(?3, notes), blocked_by = ?4, assignee = ?5, updated_at = ?6 WHERE id = ?1",
                    params![id, status, notes, blocked_by, new_assignee, now],
                )?;
            }
            None => {
                self.conn.execute(
                    "UPDATE tickets SET status = ?2, notes = COALESCE(?3, notes), blocked_by = ?4, updated_at = ?5 WHERE id = ?1",
                    params![id, status, notes, blocked_by, now],
                )?;
            }
        }
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

    // --- Ticket Keywords / Deduplication ---

    /// Extract meaningful keywords from text: lowercase, split on non-alphanumeric,
    /// filter stop words and short tokens.
    pub fn extract_keywords(text: &str) -> std::collections::HashSet<String> {
        const STOP_WORDS: &[&str] = &[
            "a", "an", "the", "and", "or", "but", "in", "on", "at", "to", "for",
            "of", "with", "by", "from", "is", "it", "as", "be", "was", "are",
            "that", "this", "not", "can", "will", "has", "have", "had", "do",
            "does", "did", "should", "would", "could", "may", "might", "shall",
            "its", "into", "than", "then", "also", "just", "so", "if", "when",
            "use", "using", "via", "e.g", "etc", "new", "all", "each", "any",
            "about", "over", "after", "before", "between", "through", "during",
        ];
        text.to_lowercase()
            .split(|c: char| !c.is_alphanumeric())
            .filter(|w| w.len() >= 3 && !STOP_WORDS.contains(w))
            .map(String::from)
            .collect()
    }

    /// Store keywords for a ticket.
    pub fn store_ticket_keywords(&self, ticket_id: &str, title: &str, description: &str) -> Result<()> {
        let mut keywords = Self::extract_keywords(title);
        keywords.extend(Self::extract_keywords(description));
        let mut stmt = self.conn.prepare(
            "INSERT OR IGNORE INTO ticket_keywords (ticket_id, keyword) VALUES (?1, ?2)"
        )?;
        for kw in &keywords {
            stmt.execute(params![ticket_id, kw])?;
        }
        Ok(())
    }

    /// Find tickets that share keywords with the given text and compute Jaccard similarity.
    /// Returns vec of (ticket_id, title, similarity_score) sorted by score descending.
    pub fn find_similar_tickets(&self, title: &str, description: &str) -> Result<Vec<(String, String, f64)>> {
        let input_keywords = Self::extract_keywords(title);
        let input_desc_keywords = Self::extract_keywords(description);
        let all_input: std::collections::HashSet<String> =
            input_keywords.union(&input_desc_keywords).cloned().collect();

        if all_input.is_empty() {
            return Ok(vec![]);
        }

        // Find candidate ticket IDs that share at least one keyword
        let placeholders: Vec<String> = (0..all_input.len()).map(|i| format!("?{}", i + 1)).collect();
        let sql = format!(
            "SELECT DISTINCT ticket_id FROM ticket_keywords WHERE keyword IN ({})",
            placeholders.join(", ")
        );
        let keyword_vec: Vec<&str> = all_input.iter().map(|s| s.as_str()).collect();
        let mut stmt = self.conn.prepare(&sql)?;
        let candidate_ids: Vec<String> = stmt
            .query_map(rusqlite::params_from_iter(&keyword_vec), |row| row.get(0))?
            .filter_map(|r| r.ok())
            .collect();

        let mut results = Vec::new();
        for cid in candidate_ids {
            // Get this candidate's keywords
            let mut kw_stmt = self.conn.prepare(
                "SELECT keyword FROM ticket_keywords WHERE ticket_id = ?1"
            )?;
            let candidate_keywords: std::collections::HashSet<String> = kw_stmt
                .query_map(params![cid], |row| row.get(0))?
                .filter_map(|r| r.ok())
                .collect();

            // Jaccard similarity
            let intersection = all_input.intersection(&candidate_keywords).count() as f64;
            let union = all_input.union(&candidate_keywords).count() as f64;
            let similarity = if union > 0.0 { intersection / union } else { 0.0 };

            if similarity > 0.0 {
                // Get ticket title
                let ticket_title: String = self.conn.query_row(
                    "SELECT title FROM tickets WHERE id = ?1",
                    params![cid],
                    |row| row.get(0),
                )?;
                results.push((cid, ticket_title, similarity));
            }
        }

        results.sort_by(|a, b| b.2.partial_cmp(&a.2).unwrap_or(std::cmp::Ordering::Equal));
        Ok(results)
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

    pub fn list_all_knowledge(&self) -> Result<Vec<KnowledgeEntry>> {
        let mut stmt = self.conn.prepare(
            "SELECT domain, key, value, version, updated_at FROM knowledge ORDER BY domain, key"
        )?;
        let rows = stmt.query_map([], |row| Ok(KnowledgeEntry {
            domain: row.get(0)?, key: row.get(1)?, value: row.get(2)?,
            version: row.get(3)?, updated_at: row.get(4)?,
        }))?;
        rows.map(|r| r.map_err(Into::into)).collect()
    }

    pub fn total_tokens_used(&self) -> Result<i64> {
        self.conn.query_row(
            "SELECT COALESCE(SUM(tokens_used), 0) FROM events",
            [],
            |row| row.get(0),
        ).map_err(Into::into)
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
            "INSERT INTO events (timestamp, agent, event_type, detail, tokens_used, input_tokens, output_tokens) VALUES (?1, ?2, ?3, ?4, ?5, 0, 0)",
            params![now, agent, event_type, detail, tokens.unwrap_or(0)],
        )?;
        Ok(())
    }

    /// Log an event with detailed token breakdown (input/output separate) and ticket/model metadata.
    pub fn log_token_event(
        &self,
        agent: Option<&str>,
        event_type: &str,
        detail: &str,
        input_tokens: i64,
        output_tokens: i64,
        ticket_id: Option<&str>,
        model: Option<&str>,
    ) -> Result<()> {
        let now = Utc::now().to_rfc3339();
        let total = input_tokens + output_tokens;
        self.conn.execute(
            "INSERT INTO events (timestamp, agent, event_type, detail, tokens_used, input_tokens, output_tokens, ticket_id, model)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![now, agent, event_type, detail, total, input_tokens, output_tokens, ticket_id, model],
        )?;
        Ok(())
    }

    pub fn recent_events(&self, limit: usize) -> Result<Vec<Event>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, timestamp, agent, event_type, detail, tokens_used, input_tokens, output_tokens, ticket_id, model
             FROM events ORDER BY id DESC LIMIT ?1"
        )?;
        let rows = stmt.query_map(params![limit as i64], |row| Ok(Event {
            id: row.get(0)?, timestamp: row.get(1)?, agent: row.get(2)?,
            event_type: row.get(3)?, detail: row.get(4)?, tokens_used: row.get(5)?,
            input_tokens: row.get(6)?, output_tokens: row.get(7)?,
            ticket_id: row.get(8)?, model: row.get(9)?,
        }))?;
        rows.map(|r| r.map_err(Into::into)).collect()
    }

    /// Returns per-ticket token usage (summed across all events for each ticket).
    pub fn token_breakdown_by_ticket(&self) -> Result<Vec<crate::models::TicketTokenUsage>> {
        let mut stmt = self.conn.prepare(
            "SELECT ticket_id, SUM(input_tokens), SUM(output_tokens)
             FROM events
             WHERE ticket_id IS NOT NULL
             GROUP BY ticket_id
             ORDER BY ticket_id"
        )?;
        let rows = stmt.query_map([], |row| {
            Ok(crate::models::TicketTokenUsage {
                ticket_id: row.get(0)?,
                input_tokens: row.get(1)?,
                output_tokens: row.get(2)?,
            })
        })?;
        rows.map(|r| r.map_err(Into::into)).collect()
    }

    // --- Milestones ---

    pub fn create_milestone(&self, name: &str, goal: &str) -> Result<i64> {
        let now = Utc::now().to_rfc3339();
        self.conn.execute(
            "INSERT INTO milestones (name, goal, status, created_at, updated_at) VALUES (?1, ?2, 'pending', ?3, ?3)",
            params![name, goal, now],
        )?;
        Ok(self.conn.last_insert_rowid())
    }

    pub fn assign_ticket_to_milestone(&self, milestone_id: i64, ticket_id: &str) -> Result<()> {
        self.conn.execute(
            "INSERT OR IGNORE INTO milestone_tickets (milestone_id, ticket_id) VALUES (?1, ?2)",
            params![milestone_id, ticket_id],
        )?;
        Ok(())
    }

    pub fn list_milestones(&self) -> Result<Vec<crate::models::Milestone>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, name, goal, status, created_at, updated_at FROM milestones ORDER BY id"
        )?;
        let rows = stmt.query_map([], |row| {
            Ok(crate::models::Milestone {
                id: row.get(0)?,
                name: row.get(1)?,
                goal: row.get(2)?,
                status: row.get(3)?,
                created_at: row.get(4)?,
                updated_at: row.get(5)?,
                tickets: vec![],
            })
        })?;
        let mut milestones: Vec<crate::models::Milestone> = rows
            .map(|r| r.map_err(Into::into))
            .collect::<Result<_>>()?;
        // Populate tickets for each milestone
        for ms in &mut milestones {
            ms.tickets = self.get_milestone_ticket_ids(ms.id)?;
        }
        Ok(milestones)
    }

    pub fn get_milestone(&self, id: i64) -> Result<Option<crate::models::Milestone>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, name, goal, status, created_at, updated_at FROM milestones WHERE id = ?1"
        )?;
        let ms = stmt.query_row(params![id], |row| {
            Ok(crate::models::Milestone {
                id: row.get(0)?,
                name: row.get(1)?,
                goal: row.get(2)?,
                status: row.get(3)?,
                created_at: row.get(4)?,
                updated_at: row.get(5)?,
                tickets: vec![],
            })
        }).optional()?;
        if let Some(mut ms) = ms {
            ms.tickets = self.get_milestone_ticket_ids(ms.id)?;
            return Ok(Some(ms));
        }
        Ok(None)
    }

    pub fn get_active_milestone(&self) -> Result<Option<crate::models::Milestone>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, name, goal, status, created_at, updated_at FROM milestones WHERE status = 'active' ORDER BY id LIMIT 1"
        )?;
        let ms = stmt.query_row([], |row| {
            Ok(crate::models::Milestone {
                id: row.get(0)?,
                name: row.get(1)?,
                goal: row.get(2)?,
                status: row.get(3)?,
                created_at: row.get(4)?,
                updated_at: row.get(5)?,
                tickets: vec![],
            })
        }).optional()?;
        if let Some(mut ms) = ms {
            ms.tickets = self.get_milestone_ticket_ids(ms.id)?;
            return Ok(Some(ms));
        }
        Ok(None)
    }

    pub fn get_awaiting_approval_milestone(&self) -> Result<Option<crate::models::Milestone>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, name, goal, status, created_at, updated_at FROM milestones WHERE status = 'awaiting_approval' ORDER BY id LIMIT 1"
        )?;
        let ms = stmt.query_row([], |row| {
            Ok(crate::models::Milestone {
                id: row.get(0)?,
                name: row.get(1)?,
                goal: row.get(2)?,
                status: row.get(3)?,
                created_at: row.get(4)?,
                updated_at: row.get(5)?,
                tickets: vec![],
            })
        }).optional()?;
        if let Some(mut ms) = ms {
            ms.tickets = self.get_milestone_ticket_ids(ms.id)?;
            return Ok(Some(ms));
        }
        Ok(None)
    }

    pub fn update_milestone_status(&self, id: i64, status: &str) -> Result<()> {
        let now = Utc::now().to_rfc3339();
        self.conn.execute(
            "UPDATE milestones SET status = ?2, updated_at = ?3 WHERE id = ?1",
            params![id, status, now],
        )?;
        Ok(())
    }

    /// Activate the first pending milestone if no milestone is currently active.
    /// Returns the activated milestone id, or None if already have an active one.
    pub fn activate_first_pending_milestone(&self) -> Result<Option<i64>> {
        // If there's already an active milestone, do nothing
        let active: Option<i64> = self.conn.query_row(
            "SELECT id FROM milestones WHERE status = 'active' LIMIT 1",
            [],
            |row| row.get(0),
        ).optional()?;
        if active.is_some() {
            return Ok(None);
        }
        // Activate the lowest-id pending milestone
        let next: Option<i64> = self.conn.query_row(
            "SELECT id FROM milestones WHERE status = 'pending' ORDER BY id LIMIT 1",
            [],
            |row| row.get(0),
        ).optional()?;
        if let Some(next_id) = next {
            self.update_milestone_status(next_id, "active")?;
            return Ok(Some(next_id));
        }
        Ok(None)
    }

    /// Approve the current awaiting_approval milestone and activate the next pending one.
    /// Returns (approved_id, next_activated_id).
    pub fn approve_milestone(&self) -> Result<Option<(i64, Option<i64>)>> {
        let ms = self.get_awaiting_approval_milestone()?;
        if let Some(ms) = ms {
            self.update_milestone_status(ms.id, "approved")?;
            let next = self.activate_first_pending_milestone()?;
            return Ok(Some((ms.id, next)));
        }
        Ok(None)
    }

    /// Reject the current awaiting_approval milestone. Sets it back to 'rejected'.
    /// Returns the rejected milestone id.
    pub fn reject_milestone(&self) -> Result<Option<i64>> {
        let ms = self.get_awaiting_approval_milestone()?;
        if let Some(ms) = ms {
            self.update_milestone_status(ms.id, "rejected")?;
            return Ok(Some(ms.id));
        }
        Ok(None)
    }

    pub fn get_milestone_ticket_ids(&self, milestone_id: i64) -> Result<Vec<String>> {
        let mut stmt = self.conn.prepare(
            "SELECT ticket_id FROM milestone_tickets WHERE milestone_id = ?1 ORDER BY ticket_id"
        )?;
        let rows = stmt.query_map(params![milestone_id], |row| row.get(0))?;
        rows.map(|r| r.map_err(Into::into)).collect()
    }

    /// Returns true when ALL tickets in the milestone are 'completed'.
    pub fn is_milestone_complete(&self, milestone_id: i64) -> Result<bool> {
        let total: i64 = self.conn.query_row(
            "SELECT COUNT(*) FROM milestone_tickets WHERE milestone_id = ?1",
            params![milestone_id],
            |row| row.get(0),
        )?;
        if total == 0 {
            return Ok(false);
        }
        let not_done: i64 = self.conn.query_row(
            "SELECT COUNT(*) FROM milestone_tickets mt
             JOIN tickets t ON mt.ticket_id = t.id
             WHERE mt.milestone_id = ?1 AND t.status != 'completed'",
            params![milestone_id],
            |row| row.get(0),
        )?;
        Ok(not_done == 0)
    }

    /// Check whether any milestones exist in the DB.
    pub fn has_milestones(&self) -> Result<bool> {
        let count: i64 = self.conn.query_row(
            "SELECT COUNT(*) FROM milestones",
            [],
            |row| row.get(0),
        )?;
        Ok(count > 0)
    }

    /// Claim the next pending ticket within a specific milestone for an agent.
    pub fn claim_next_milestone_ticket(&self, agent_id: &str, milestone_id: i64) -> Result<Option<Ticket>> {
        let now = Utc::now().to_rfc3339();
        let tx = self.conn.unchecked_transaction()?;
        let ticket: Option<Ticket> = tx.query_row(
            "UPDATE tickets SET status = 'in_progress', assignee = ?1, updated_at = ?2
             WHERE id = (
                 SELECT t.id FROM tickets t
                 JOIN milestone_tickets mt ON t.id = mt.ticket_id
                 WHERE t.status = 'pending' AND t.assignee IS NULL AND mt.milestone_id = ?3
                 ORDER BY t.priority, t.created_at LIMIT 1
             )
             RETURNING id, title, description, domain, priority, status, assignee, blocked_by, notes, created_at, updated_at",
            params![agent_id, now, milestone_id],
            Self::row_to_ticket,
        ).optional()?;
        tx.commit()?;
        Ok(ticket)
    }

    /// Returns total (input_tokens, output_tokens) across all events.
    pub fn total_token_details(&self) -> Result<(i64, i64)> {
        self.conn.query_row(
            "SELECT COALESCE(SUM(input_tokens), 0), COALESCE(SUM(output_tokens), 0) FROM events",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        ).map_err(Into::into)
    }
}
