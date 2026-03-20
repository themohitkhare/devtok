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
    pub const CURRENT_SCHEMA_VERSION: i64 = 5;

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
        // Tracks DB schema state so we can safely evolve migrations over time.
        // (Ticket t-062 / DB2)
        let now = Utc::now().to_rfc3339();
        self.conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS schema_meta (
                id INTEGER PRIMARY KEY CHECK (id = 1),
                schema_version INTEGER NOT NULL,
                updated_at TEXT NOT NULL
            );",
        )?;
        self.conn.execute(
            "INSERT OR IGNORE INTO schema_meta (id, schema_version, updated_at) VALUES (1, 1, ?1)",
            params![now],
        )?;
        let current_version: i64 = self
            .conn
            .query_row(
                "SELECT schema_version FROM schema_meta WHERE id = 1",
                [],
                |row| row.get(0),
            )
            .unwrap_or(1);

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
                last_heartbeat TEXT,
                backend TEXT NOT NULL DEFAULT 'claude'
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
            CREATE INDEX IF NOT EXISTS idx_milestone_tickets_milestone ON milestone_tickets(milestone_id);
            CREATE TABLE IF NOT EXISTS quality_scores (
                ticket_id TEXT PRIMARY KEY,
                tests_added INTEGER NOT NULL,
                docs_updated INTEGER NOT NULL,
                acceptance_criteria_met INTEGER NOT NULL,
                score INTEGER NOT NULL,
                computed_at TEXT NOT NULL
            );"
        )?;

        if current_version < Self::CURRENT_SCHEMA_VERSION {
            // Additive migrations: add new columns to existing tables.
            // SQLite returns an error when the column already exists; we ignore it.
            let _ = self.conn.execute_batch(
                "ALTER TABLE events ADD COLUMN input_tokens INTEGER DEFAULT 0;
                 ALTER TABLE events ADD COLUMN output_tokens INTEGER DEFAULT 0;
                 ALTER TABLE events ADD COLUMN ticket_id TEXT;
                 ALTER TABLE events ADD COLUMN model TEXT;",
            );

            // Rate-limit tracking columns on tickets.
            let _ = self.conn.execute_batch(
                "ALTER TABLE tickets ADD COLUMN rate_limit_strikes INTEGER NOT NULL DEFAULT 0;
                 ALTER TABLE tickets ADD COLUMN rate_limit_retry_after TEXT;",
            );

            // File conflict prevention: optional comma-separated file paths hint (v3).
            let _ = self.conn.execute_batch(
                "ALTER TABLE tickets ADD COLUMN files_hint TEXT;",
            );
        }

        if current_version < 4 {
            // Conflict-deferral counter: tracks how many times a ticket has been
            // skipped due to file overlap. Used for force-assign liveness guarantee (v4).
            let _ = self.conn.execute_batch(
                "ALTER TABLE tickets ADD COLUMN defer_count INTEGER NOT NULL DEFAULT 0;",
            );
            // v3: backend column for agents — restored after being dropped from migrations.
            // INSERT OR IGNORE so existing rows keep their value; new rows get 'claude'.
            let _ = self.conn.execute_batch(
                "ALTER TABLE agents ADD COLUMN backend TEXT NOT NULL DEFAULT 'claude';",
            );
        }

        if current_version < 4 {
            // v4: conflict-deferral counter for tickets.
            let _ = self.conn.execute_batch(
                "ALTER TABLE tickets ADD COLUMN defer_count INTEGER NOT NULL DEFAULT 0;",
            );
        }

        if current_version < 5 {
            // Backend label for agents: tracks which AI provider handles each worker (v5).
            let _ = self.conn.execute_batch(
                "ALTER TABLE agents ADD COLUMN backend TEXT NOT NULL DEFAULT 'claude';",
            );
        }

        self.conn.execute(
            "UPDATE schema_meta SET schema_version = ?1, updated_at = ?2 WHERE id = 1",
            params![Self::CURRENT_SCHEMA_VERSION, now],
        )?;

        // Additive migrations: rate-limit deferral columns for tickets.
        let _ = self.conn.execute_batch(
            "ALTER TABLE tickets ADD COLUMN rate_limit_retry_after TEXT;
             ALTER TABLE tickets ADD COLUMN rate_limit_strikes INTEGER NOT NULL DEFAULT 0;",
        );

        Ok(())
    }

    // --- Tickets ---

    pub fn create_ticket(
        &self,
        title: &str,
        desc: &str,
        domain: &str,
        priority: i32,
    ) -> Result<String> {
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
            "SELECT id, title, description, domain, priority, status, assignee, blocked_by, notes, created_at, updated_at,
                    COALESCE(defer_count, 0), files_hint
             FROM tickets WHERE id = ?1"
        )?;
        let ticket = stmt
            .query_row(params![id], Self::row_to_ticket)
            .optional()?;
        Ok(ticket)
    }

    pub fn list_tickets(&self, status: Option<&str>) -> Result<Vec<Ticket>> {
        let sql = match status {
            Some(_) => "SELECT id, title, description, domain, priority, status, assignee, blocked_by, notes, created_at, updated_at, COALESCE(defer_count, 0), files_hint FROM tickets WHERE status = ?1 ORDER BY priority, created_at",
            None => "SELECT id, title, description, domain, priority, status, assignee, blocked_by, notes, created_at, updated_at, COALESCE(defer_count, 0), files_hint FROM tickets ORDER BY priority, created_at",
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

    /// Re-queue a ticket as pending with exponential backoff after a rate-limit error.
    ///
    /// Backoff schedule (capped at 240 s): 30 s → 60 s → 120 s → 240 s.
    /// `current_strikes` is the strike count *before* this call; the method increments it.
    pub fn requeue_ticket_rate_limited(&self, id: &str, current_strikes: i32) -> Result<()> {
        let new_strikes = current_strikes + 1;
        // Backoff: 30 * 2^(strikes-1), capped at 240 s
        let backoff_secs: u64 = (30u64 * (1u64 << (new_strikes - 1).min(3))).min(240);
        let retry_after =
            (Utc::now() + chrono::Duration::seconds(backoff_secs as i64)).to_rfc3339();
        let notes = format!("rate_limited:{}", retry_after);
        let now = Utc::now().to_rfc3339();
        self.conn.execute(
            "UPDATE tickets SET status = 'pending', notes = ?2, rate_limit_strikes = ?3, rate_limit_retry_after = ?4, assignee = NULL, updated_at = ?5 WHERE id = ?1",
            params![id, notes, new_strikes, retry_after, now],
        )?;
        Ok(())
    }

    /// Clear rate-limit state after a ticket completes or is unblocked normally.
    pub fn clear_ticket_rate_limit_state(&self, id: &str) -> Result<()> {
        let now = Utc::now().to_rfc3339();
        self.conn.execute(
            "UPDATE tickets SET rate_limit_strikes = 0, rate_limit_retry_after = NULL, updated_at = ?2 WHERE id = ?1",
            params![id, now],
        )?;
        Ok(())
    }

    /// Returns the current rate-limit strike count for a ticket.
    pub fn get_ticket_rate_limit_strikes(&self, id: &str) -> Result<i32> {
        self.conn
            .query_row(
                "SELECT COALESCE(rate_limit_strikes, 0) FROM tickets WHERE id = ?1",
                params![id],
                |row| row.get(0),
            )
            .map_err(Into::into)
    }

    /// Returns the rate_limit_retry_after timestamp for a ticket, if set.
    pub fn get_ticket_rate_limit_retry_after(
        &self,
        id: &str,
    ) -> Result<Option<chrono::DateTime<chrono::Utc>>> {
        let ts: Option<String> = self
            .conn
            .query_row(
                "SELECT rate_limit_retry_after FROM tickets WHERE id = ?1",
                params![id],
                |row| row.get(0),
            )
            .optional()?
            .flatten();
        match ts {
            None => Ok(None),
            Some(s) => {
                let dt = chrono::DateTime::parse_from_rfc3339(&s)
                    .map(|d| d.with_timezone(&chrono::Utc))
                    .map_err(|e| anyhow::anyhow!("invalid retry_after timestamp: {}", e))?;
                Ok(Some(dt))
            }
        }
    }

    /// Store computed file-path hints for a ticket (comma-separated).
    /// Used by the manager's conflict-prevention check.
    pub fn set_ticket_files_hint(&self, id: &str, hints: &[String]) -> Result<()> {
        let hint_str = hints.join(",");
        let now = Utc::now().to_rfc3339();
        self.conn.execute(
            "UPDATE tickets SET files_hint = ?2, updated_at = ?3 WHERE id = ?1",
            params![id, hint_str, now],
        )?;
        Ok(())
    }

    /// Retrieve stored file-path hints for a ticket, if any.
    pub fn get_ticket_files_hint(&self, id: &str) -> Result<Option<Vec<String>>> {
        let raw: Option<String> = self
            .conn
            .query_row(
                "SELECT files_hint FROM tickets WHERE id = ?1",
                params![id],
                |row| row.get(0),
            )
            .optional()?
            .flatten();
        Ok(raw.map(|s| {
            if s.is_empty() {
                vec![]
            } else {
                s.split(',').map(str::to_string).collect()
            }
        }))
    }

    /// Increment the conflict-deferral counter for a ticket and return the new value.
    pub fn increment_defer_count(&self, id: &str) -> Result<i32> {
        let now = Utc::now().to_rfc3339();
        let new_count: i32 = self.conn.query_row(
            "UPDATE tickets SET defer_count = COALESCE(defer_count, 0) + 1, updated_at = ?2
             WHERE id = ?1
             RETURNING COALESCE(defer_count, 0)",
            params![id, now],
            |row| row.get(0),
        )?;
        Ok(new_count)
    }

    /// Reset the conflict-deferral counter for a ticket to zero.
    pub fn reset_defer_count(&self, id: &str) -> Result<()> {
        let now = Utc::now().to_rfc3339();
        self.conn.execute(
            "UPDATE tickets SET defer_count = 0, updated_at = ?2 WHERE id = ?1",
            params![id, now],
        )?;
        Ok(())
    }

    /// List tickets whose defer_count exceeds the given threshold, ordered by defer_count desc.
    pub fn list_tickets_with_defer_count_gt(&self, threshold: i32) -> Result<Vec<Ticket>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, title, description, domain, priority, status, assignee, blocked_by, notes, created_at, updated_at,
                    COALESCE(defer_count, 0), files_hint
             FROM tickets WHERE COALESCE(defer_count, 0) > ?1 ORDER BY defer_count DESC, created_at"
        )?;
        let rows = stmt.query_map(params![threshold], Self::row_to_ticket)?;
        rows.map(|r| r.map_err(Into::into)).collect()
    }

    /// Atomically claim a specific pending ticket by ID for an agent.
    ///
    /// Returns `None` if the ticket is not claimable (already taken, not pending,
    /// or still within its rate-limit window).
    pub fn claim_ticket_by_id(&self, ticket_id: &str, agent_id: &str) -> Result<Option<Ticket>> {
        let now = Utc::now().to_rfc3339();
        let tx = self.conn.unchecked_transaction()?;
        let ticket: Option<Ticket> = tx.query_row(
            "UPDATE tickets SET status = 'in_progress', assignee = ?1, updated_at = ?2, rate_limit_retry_after = NULL, defer_count = 0
             WHERE id = ?3 AND status = 'pending'
               AND (rate_limit_retry_after IS NULL OR rate_limit_retry_after <= ?2)
             RETURNING id, title, description, domain, priority, status, assignee, blocked_by, notes, created_at, updated_at,
                       COALESCE(defer_count, 0), files_hint",
            params![agent_id, now, ticket_id],
            Self::row_to_ticket,
        ).optional()?;
        tx.commit()?;
        Ok(ticket)
    }

    /// Peek at the next pending ticket without claiming it.
    /// Used by the manager for conflict detection before deciding to assign or defer.
    pub fn peek_next_pending_ticket(&self) -> Result<Option<Ticket>> {
        let now = Utc::now().to_rfc3339();
        self.conn.query_row(
            "SELECT id, title, description, domain, priority, status, assignee, blocked_by, notes, created_at, updated_at,
                    COALESCE(defer_count, 0), files_hint
             FROM tickets
             WHERE status = 'pending' AND assignee IS NULL
               AND (rate_limit_retry_after IS NULL OR rate_limit_retry_after <= ?1)
             ORDER BY priority, created_at LIMIT 1",
            params![now],
            Self::row_to_ticket,
        ).optional().map_err(Into::into)
    }

    /// Set the files_hint for a ticket (comma-separated file paths).
    pub fn set_files_hint(&self, ticket_id: &str, hint: &str) -> Result<()> {
        let now = Utc::now().to_rfc3339();
        self.conn.execute(
            "UPDATE tickets SET files_hint = ?2, updated_at = ?3 WHERE id = ?1",
            params![ticket_id, hint, now],
        )?;
        Ok(())
    }

    /// List tickets with defer_count >= min_defer_count (the "stuck" tickets).
    pub fn list_stuck_tickets(&self, min_defer_count: i32) -> Result<Vec<Ticket>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, title, description, domain, priority, status, assignee, blocked_by, notes, created_at, updated_at,
                    COALESCE(defer_count, 0), files_hint
             FROM tickets
             WHERE COALESCE(defer_count, 0) >= ?1
             ORDER BY defer_count DESC, priority, created_at",
        )?;
        let rows = stmt.query_map(params![min_defer_count], Self::row_to_ticket)?;
        rows.map(|r| r.map_err(Into::into)).collect()
    }

    /// Create a ticket with an explicit ID (used in tests).
    #[cfg(test)]
    pub fn create_ticket_with_id(
        &self,
        id: &str,
        title: &str,
        desc: &str,
        domain: &str,
        priority: i32,
    ) -> Result<()> {
        let now = Utc::now().to_rfc3339();
        self.conn.execute(
            "INSERT OR IGNORE INTO tickets (id, title, description, domain, priority, status, notes, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, 'pending', '', ?6, ?6)",
            params![id, title, desc, domain, priority, now],
        )?;
        Ok(())
    }

    pub fn claim_next_ticket(&self, agent_id: &str) -> Result<Option<Ticket>> {
        let now = Utc::now().to_rfc3339();
        let tx = self.conn.unchecked_transaction()?;
        let ticket: Option<Ticket> = tx.query_row(
            "UPDATE tickets SET status = 'in_progress', assignee = ?1, updated_at = ?2, rate_limit_retry_after = NULL, defer_count = 0
             WHERE id = (
                 SELECT id FROM tickets
                 WHERE status = 'pending' AND assignee IS NULL
                   AND (rate_limit_retry_after IS NULL OR rate_limit_retry_after <= ?2)
                 ORDER BY priority, created_at LIMIT 1
             )
             RETURNING id, title, description, domain, priority, status, assignee, blocked_by, notes, created_at, updated_at,
                       COALESCE(defer_count, 0), files_hint",
            params![agent_id, now],
            Self::row_to_ticket,
        ).optional()?;
        tx.commit()?;
        Ok(ticket)
    }

    /// Force-set the `rate_limit_retry_after` timestamp for a ticket (used in tests).
    ///
    /// Pass `None` to clear the field, or `Some(dt)` to set it to any arbitrary
    /// timestamp — including one in the past — so tests can simulate an expired
    /// retry window without sleeping.
    #[cfg(test)]
    pub fn force_set_rate_limit_retry_after(
        &self,
        id: &str,
        retry_after: Option<chrono::DateTime<chrono::Utc>>,
    ) -> Result<()> {
        let now = Utc::now().to_rfc3339();
        let ts = retry_after.map(|dt| dt.to_rfc3339());
        self.conn.execute(
            "UPDATE tickets SET rate_limit_retry_after = ?2, updated_at = ?3 WHERE id = ?1",
            params![id, ts, now],
        )?;
        Ok(())
    }

    pub fn count_by_status(&self) -> Result<Vec<(String, i64)>> {
        let mut stmt = self
            .conn
            .prepare("SELECT status, COUNT(*) FROM tickets GROUP BY status")?;
        let rows = stmt.query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?;
        rows.map(|r| r.map_err(Into::into)).collect()
    }

    fn row_to_ticket(row: &rusqlite::Row) -> rusqlite::Result<Ticket> {
        Ok(Ticket {
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
            defer_count: row.get(11)?,
            files_hint: row.get(12)?,
        })
    }

    // --- Ticket Keywords / Deduplication ---

    /// Extract meaningful keywords from text: lowercase, split on non-alphanumeric,
    /// filter stop words and short tokens.
    pub fn extract_keywords(text: &str) -> std::collections::HashSet<String> {
        const STOP_WORDS: &[&str] = &[
            "a", "an", "the", "and", "or", "but", "in", "on", "at", "to", "for", "of", "with",
            "by", "from", "is", "it", "as", "be", "was", "are", "that", "this", "not", "can",
            "will", "has", "have", "had", "do", "does", "did", "should", "would", "could", "may",
            "might", "shall", "its", "into", "than", "then", "also", "just", "so", "if", "when",
            "use", "using", "via", "e.g", "etc", "new", "all", "each", "any", "about", "over",
            "after", "before", "between", "through", "during",
        ];
        text.to_lowercase()
            .split(|c: char| !c.is_alphanumeric())
            .filter(|w| w.len() >= 3 && !STOP_WORDS.contains(w))
            .map(String::from)
            .collect()
    }

    /// Store keywords for a ticket.
    pub fn store_ticket_keywords(
        &self,
        ticket_id: &str,
        title: &str,
        description: &str,
    ) -> Result<()> {
        let mut keywords = Self::extract_keywords(title);
        keywords.extend(Self::extract_keywords(description));
        let mut stmt = self.conn.prepare(
            "INSERT OR IGNORE INTO ticket_keywords (ticket_id, keyword) VALUES (?1, ?2)",
        )?;
        for kw in &keywords {
            stmt.execute(params![ticket_id, kw])?;
        }
        Ok(())
    }

    /// Find tickets that share keywords with the given text and compute Jaccard similarity.
    /// Returns vec of (ticket_id, title, similarity_score) sorted by score descending.
    pub fn find_similar_tickets(
        &self,
        title: &str,
        description: &str,
    ) -> Result<Vec<(String, String, f64)>> {
        let input_keywords = Self::extract_keywords(title);
        let input_desc_keywords = Self::extract_keywords(description);
        let all_input: std::collections::HashSet<String> = input_keywords
            .union(&input_desc_keywords)
            .cloned()
            .collect();

        if all_input.is_empty() {
            return Ok(vec![]);
        }

        // Find candidate ticket IDs that share at least one keyword
        let placeholders: Vec<String> = (0..all_input.len())
            .map(|i| format!("?{}", i + 1))
            .collect();
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
            let mut kw_stmt = self
                .conn
                .prepare("SELECT keyword FROM ticket_keywords WHERE ticket_id = ?1")?;
            let candidate_keywords: std::collections::HashSet<String> = kw_stmt
                .query_map(params![cid], |row| row.get(0))?
                .filter_map(|r| r.ok())
                .collect();

            // Jaccard similarity
            let intersection = all_input.intersection(&candidate_keywords).count() as f64;
            let union = all_input.union(&candidate_keywords).count() as f64;
            let similarity = if union > 0.0 {
                intersection / union
            } else {
                0.0
            };

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

    pub fn push_inbox(
        &self,
        recipient: &str,
        msg_type: &str,
        payload: &str,
        sender: &str,
    ) -> Result<()> {
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
            "SELECT domain, key, value, version, updated_at FROM knowledge ORDER BY domain, key",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok(KnowledgeEntry {
                domain: row.get(0)?,
                key: row.get(1)?,
                value: row.get(2)?,
                version: row.get(3)?,
                updated_at: row.get(4)?,
            })
        })?;
        rows.map(|r| r.map_err(Into::into)).collect()
    }

    pub fn list_knowledge_by_domain(&self, domain: &str) -> Result<Vec<KnowledgeEntry>> {
        let mut stmt = self.conn.prepare(
            "SELECT domain, key, value, version, updated_at FROM knowledge WHERE domain = ?1 ORDER BY key",
        )?;
        let rows = stmt.query_map(params![domain], |row| {
            Ok(KnowledgeEntry {
                domain: row.get(0)?,
                key: row.get(1)?,
                value: row.get(2)?,
                version: row.get(3)?,
                updated_at: row.get(4)?,
            })
        })?;
        rows.map(|r| r.map_err(Into::into)).collect()
    }

    pub fn total_tokens_used(&self) -> Result<i64> {
        self.conn
            .query_row(
                "SELECT COALESCE(SUM(tokens_used), 0) FROM events",
                [],
                |row| row.get(0),
            )
            .map_err(Into::into)
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
            "INSERT OR REPLACE INTO agents (id, role, persona, status, backend) VALUES (?1, ?2, ?3, 'idle', 'claude')",
            params![id, role, persona],
        )?;
        Ok(())
    }

    /// Register an agent with an explicit backend label (e.g. "claude", "cursor", "codex").
    pub fn register_agent_with_backend(
        &self,
        id: &str,
        role: &str,
        persona: &str,
        backend: &str,
    ) -> Result<()> {
        self.conn.execute(
            "INSERT OR REPLACE INTO agents (id, role, persona, status, backend) VALUES (?1, ?2, ?3, 'idle', ?4)",
            params![id, role, persona, backend],
        )?;
        Ok(())
    }

    pub fn update_agent(
        &self,
        id: &str,
        status: &str,
        ticket: Option<&str>,
        pid: Option<u32>,
    ) -> Result<()> {
        let now = Utc::now().to_rfc3339();
        self.conn.execute(
            "UPDATE agents SET status = ?2, current_ticket = ?3, pid = ?4, last_heartbeat = ?5 WHERE id = ?1",
            params![id, status, ticket, pid, now],
        )?;
        Ok(())
    }

    pub fn list_agents(&self) -> Result<Vec<Agent>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, role, persona, status, current_ticket, pid, last_heartbeat, backend FROM agents",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok(Agent {
                id: row.get(0)?,
                role: row.get(1)?,
                persona: row.get(2)?,
                status: row.get(3)?,
                current_ticket: row.get(4)?,
                pid: row.get(5)?,
                last_heartbeat: row.get(6)?,
                backend: row.get::<_, Option<String>>(7)?.unwrap_or_else(|| "claude".to_string()),
            })
        })?;
        rows.map(|r| r.map_err(Into::into)).collect()
    }

    pub fn deregister_agent(&self, id: &str) -> Result<()> {
        self.conn
            .execute("DELETE FROM agents WHERE id = ?1", params![id])?;
        Ok(())
    }

    // --- Events ---

    pub fn log_event(
        &self,
        agent: Option<&str>,
        event_type: &str,
        detail: &str,
        tokens: Option<i64>,
    ) -> Result<()> {
        let now = Utc::now().to_rfc3339();
        self.conn.execute(
            "INSERT INTO events (timestamp, agent, event_type, detail, tokens_used, input_tokens, output_tokens) VALUES (?1, ?2, ?3, ?4, ?5, 0, 0)",
            params![now, agent, event_type, detail, tokens.unwrap_or(0)],
        )?;
        Ok(())
    }

    /// Log an event with detailed token breakdown (input/output separate) and ticket/model metadata.
    #[allow(clippy::too_many_arguments)]
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
        let rows = stmt.query_map(params![limit as i64], |row| {
            Ok(Event {
                id: row.get(0)?,
                timestamp: row.get(1)?,
                agent: row.get(2)?,
                event_type: row.get(3)?,
                detail: row.get(4)?,
                tokens_used: row.get(5)?,
                input_tokens: row.get(6)?,
                output_tokens: row.get(7)?,
                ticket_id: row.get(8)?,
                model: row.get(9)?,
            })
        })?;
        rows.map(|r| r.map_err(Into::into)).collect()
    }

    /// Return events of a specific type, most recent first, up to `limit`.
    pub fn list_recent_events_of_type(&self, event_type: &str, limit: usize) -> Result<Vec<Event>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, timestamp, agent, event_type, detail, tokens_used, input_tokens, output_tokens, ticket_id, model
             FROM events
             WHERE event_type = ?1
             ORDER BY id DESC
             LIMIT ?2",
        )?;
        let rows = stmt.query_map(params![event_type, limit as i64], |row| {
            Ok(Event {
                id: row.get(0)?,
                timestamp: row.get(1)?,
                agent: row.get(2)?,
                event_type: row.get(3)?,
                detail: row.get(4)?,
                tokens_used: row.get(5)?,
                input_tokens: row.get(6)?,
                output_tokens: row.get(7)?,
                ticket_id: row.get(8)?,
                model: row.get(9)?,
            })
        })?;
        rows.map(|r| r.map_err(Into::into)).collect()
    }

    pub fn recent_events_for_agent(&self, agent: &str, limit: usize) -> Result<Vec<Event>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, timestamp, agent, event_type, detail, tokens_used, input_tokens, output_tokens, ticket_id, model
             FROM events
             WHERE agent = ?1
             ORDER BY id DESC
             LIMIT ?2",
        )?;
        let rows = stmt.query_map(params![agent, limit as i64], |row| {
            Ok(Event {
                id: row.get(0)?,
                timestamp: row.get(1)?,
                agent: row.get(2)?,
                event_type: row.get(3)?,
                detail: row.get(4)?,
                tokens_used: row.get(5)?,
                input_tokens: row.get(6)?,
                output_tokens: row.get(7)?,
                ticket_id: row.get(8)?,
                model: row.get(9)?,
            })
        })?;
        rows.map(|r| r.map_err(Into::into)).collect()
    }

    /// Returns per-ticket token usage (summed across all events for each ticket).
    pub fn token_breakdown_by_ticket(&self) -> Result<Vec<crate::models::TicketTokenUsage>> {
        let mut stmt = self.conn.prepare(
            "SELECT ticket_id, SUM(input_tokens), SUM(output_tokens)
             FROM events
             WHERE ticket_id IS NOT NULL
             GROUP BY ticket_id
             ORDER BY ticket_id",
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

    pub fn recent_events_filtered(
        &self,
        worker: Option<&str>,
        ticket: Option<&str>,
        limit: usize,
    ) -> Result<Vec<Event>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, timestamp, agent, event_type, detail, tokens_used, input_tokens, output_tokens, ticket_id, model
             FROM events
             WHERE (?1 IS NULL OR agent = ?1)
               AND (?2 IS NULL OR ticket_id = ?2)
             ORDER BY id DESC
             LIMIT ?3",
        )?;
        let rows = stmt.query_map(params![worker, ticket, limit as i64], |row| {
            Ok(Event {
                id: row.get(0)?,
                timestamp: row.get(1)?,
                agent: row.get(2)?,
                event_type: row.get(3)?,
                detail: row.get(4)?,
                tokens_used: row.get(5)?,
                input_tokens: row.get(6)?,
                output_tokens: row.get(7)?,
                ticket_id: row.get(8)?,
                model: row.get(9)?,
            })
        })?;
        rows.map(|r| r.map_err(Into::into)).collect()
    }

    /// Returns the RFC-3339 timestamp of the most recent `binary_rebuilt` event,
    /// or `None` if no rebuild has ever been recorded.
    pub fn last_rebuilt_at(&self) -> Result<Option<String>> {
        let mut stmt = self.conn.prepare(
            "SELECT timestamp FROM events WHERE event_type = 'binary_rebuilt' ORDER BY id DESC LIMIT 1",
        )?;
        let mut rows = stmt.query([])?;
        if let Some(row) = rows.next()? {
            let ts: String = row.get(0)?;
            Ok(Some(ts))
        } else {
            Ok(None)
        }
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
            "SELECT id, name, goal, status, created_at, updated_at FROM milestones ORDER BY id",
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
        let mut milestones: Vec<crate::models::Milestone> =
            rows.map(|r| r.map_err(Into::into)).collect::<Result<_>>()?;
        // Populate tickets for each milestone
        for ms in &mut milestones {
            ms.tickets = self.get_milestone_ticket_ids(ms.id)?;
        }
        Ok(milestones)
    }

    pub fn get_milestone(&self, id: i64) -> Result<Option<crate::models::Milestone>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, name, goal, status, created_at, updated_at FROM milestones WHERE id = ?1",
        )?;
        let ms = stmt
            .query_row(params![id], |row| {
                Ok(crate::models::Milestone {
                    id: row.get(0)?,
                    name: row.get(1)?,
                    goal: row.get(2)?,
                    status: row.get(3)?,
                    created_at: row.get(4)?,
                    updated_at: row.get(5)?,
                    tickets: vec![],
                })
            })
            .optional()?;
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
        let ms = stmt
            .query_row([], |row| {
                Ok(crate::models::Milestone {
                    id: row.get(0)?,
                    name: row.get(1)?,
                    goal: row.get(2)?,
                    status: row.get(3)?,
                    created_at: row.get(4)?,
                    updated_at: row.get(5)?,
                    tickets: vec![],
                })
            })
            .optional()?;
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
        let ms = stmt
            .query_row([], |row| {
                Ok(crate::models::Milestone {
                    id: row.get(0)?,
                    name: row.get(1)?,
                    goal: row.get(2)?,
                    status: row.get(3)?,
                    created_at: row.get(4)?,
                    updated_at: row.get(5)?,
                    tickets: vec![],
                })
            })
            .optional()?;
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
        let active: Option<i64> = self
            .conn
            .query_row(
                "SELECT id FROM milestones WHERE status = 'active' LIMIT 1",
                [],
                |row| row.get(0),
            )
            .optional()?;
        if active.is_some() {
            return Ok(None);
        }
        // Activate the lowest-id pending milestone
        let next: Option<i64> = self
            .conn
            .query_row(
                "SELECT id FROM milestones WHERE status = 'pending' ORDER BY id LIMIT 1",
                [],
                |row| row.get(0),
            )
            .optional()?;
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
            "SELECT ticket_id FROM milestone_tickets WHERE milestone_id = ?1 ORDER BY ticket_id",
        )?;
        let rows = stmt.query_map(params![milestone_id], |row| row.get(0))?;
        rows.map(|r| r.map_err(Into::into)).collect()
    }

    /// Returns true when ALL tickets in the milestone are terminal
    /// (`completed`, `cancelled`, `wont_fix`).
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
             WHERE mt.milestone_id = ?1
               AND t.status NOT IN ('completed', 'cancelled', 'wont_fix')",
            params![milestone_id],
            |row| row.get(0),
        )?;
        Ok(not_done == 0)
    }

    /// Check whether any milestones exist in the DB.
    pub fn has_milestones(&self) -> Result<bool> {
        let count: i64 = self
            .conn
            .query_row("SELECT COUNT(*) FROM milestones", [], |row| row.get(0))?;
        Ok(count > 0)
    }

    /// Claim the next pending ticket within a specific milestone for an agent.
    pub fn claim_next_milestone_ticket(
        &self,
        agent_id: &str,
        milestone_id: i64,
    ) -> Result<Option<Ticket>> {
        let now = Utc::now().to_rfc3339();
        let tx = self.conn.unchecked_transaction()?;
        let ticket: Option<Ticket> = tx.query_row(
            "UPDATE tickets SET status = 'in_progress', assignee = ?1, updated_at = ?2, rate_limit_retry_after = NULL, defer_count = 0
             WHERE id = (
                 SELECT t.id FROM tickets t
                 JOIN milestone_tickets mt ON t.id = mt.ticket_id
                 WHERE t.status = 'pending' AND t.assignee IS NULL AND mt.milestone_id = ?3
                   AND (t.rate_limit_retry_after IS NULL OR t.rate_limit_retry_after <= ?2)
                 ORDER BY t.priority, t.created_at LIMIT 1
             )
             RETURNING id, title, description, domain, priority, status, assignee, blocked_by, notes, created_at, updated_at,
                       COALESCE(defer_count, 0), files_hint",
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

    // --- Quality Scores ---

    pub fn upsert_quality_score(&self, score: &QualityScore) -> Result<()> {
        self.conn.execute(
            "INSERT INTO quality_scores (ticket_id, tests_added, docs_updated, acceptance_criteria_met, score, computed_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)
             ON CONFLICT(ticket_id) DO UPDATE SET
                 tests_added = excluded.tests_added,
                 docs_updated = excluded.docs_updated,
                 acceptance_criteria_met = excluded.acceptance_criteria_met,
                 score = excluded.score,
                 computed_at = excluded.computed_at",
            params![
                score.ticket_id,
                score.tests_added as i32,
                score.docs_updated as i32,
                score.acceptance_criteria_met as i32,
                score.score,
                score.computed_at
            ],
        )?;
        Ok(())
    }

    pub fn get_quality_score(&self, ticket_id: &str) -> Result<Option<QualityScore>> {
        self.conn
            .query_row(
                "SELECT ticket_id, tests_added, docs_updated, acceptance_criteria_met, score, computed_at
                 FROM quality_scores
                 WHERE ticket_id = ?1",
                params![ticket_id],
                Self::row_to_quality_score,
            )
            .optional()
            .map_err(Into::into)
    }

    pub fn list_quality_scores(&self) -> Result<Vec<QualityScore>> {
        let mut stmt = self.conn.prepare(
            "SELECT ticket_id, tests_added, docs_updated, acceptance_criteria_met, score, computed_at
             FROM quality_scores
             ORDER BY computed_at DESC, ticket_id ASC",
        )?;
        let rows = stmt.query_map([], Self::row_to_quality_score)?;
        rows.map(|r| r.map_err(Into::into)).collect()
    }

    fn row_to_quality_score(row: &rusqlite::Row) -> rusqlite::Result<QualityScore> {
        Ok(QualityScore {
            ticket_id: row.get(0)?,
            tests_added: row.get::<_, i32>(1)? != 0,
            docs_updated: row.get::<_, i32>(2)? != 0,
            acceptance_criteria_met: row.get::<_, i32>(3)? != 0,
            score: row.get(4)?,
            computed_at: row.get(5)?,
        })
    }
}

impl Db {
    /// Tickets completed (status = 'completed') with updated_at >= since_rfc3339.
    pub fn tickets_completed_since(&self, since_rfc3339: &str) -> Result<Vec<Ticket>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, title, description, domain, priority, status, assignee, blocked_by, notes, created_at, updated_at, \
             COALESCE(defer_count, 0), files_hint FROM tickets WHERE status = 'completed' AND updated_at >= ?1 ORDER BY updated_at DESC",
        )?;
        let rows = stmt.query_map(params![since_rfc3339], Self::row_to_ticket)?;
        rows.map(|r| r.map_err(Into::into)).collect()
    }

    /// Rolling 7-day ticket velocity: tickets completed in last 7 days / 7.
    pub fn velocity_7day(&self) -> Result<f64> {
        let since = (Utc::now() - chrono::Duration::days(7)).to_rfc3339();
        let count: i64 = self.conn.query_row(
            "SELECT COUNT(*) FROM tickets WHERE status = 'completed' AND updated_at >= ?1",
            params![since],
            |row| row.get(0),
        )?;
        Ok(count as f64 / 7.0)
    }

    /// Sum of input+output tokens for events with timestamp >= since_rfc3339.
    pub fn token_details_since(&self, since_rfc3339: &str) -> Result<(i64, i64)> {
        let (input, output) = self.conn.query_row(
            "SELECT COALESCE(SUM(input_tokens), 0), COALESCE(SUM(output_tokens), 0) FROM events WHERE timestamp >= ?1",
            params![since_rfc3339],
            |row| Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?)),
        )?;
        Ok((input, output))
    }

    /// Timestamp when a ticket was last assigned (from ticket_assignment events).
    pub fn ticket_assigned_at(&self, ticket_id: &str) -> Result<Option<String>> {
        let ts = self.conn.query_row(
            "SELECT timestamp FROM events WHERE event_type = 'ticket_assignment' AND ticket_id = ?1 ORDER BY id DESC LIMIT 1",
            params![ticket_id],
            |row| row.get(0),
        ).optional()?;
        Ok(ts)
    }
}

impl Db {
    pub fn schema_version(&self) -> Result<i64> {
        self.conn
            .query_row(
                "SELECT schema_version FROM schema_meta WHERE id = 1",
                [],
                |row| row.get(0),
            )
            .map_err(Into::into)
    }

    /// Compute rolling throughput metrics over the last 60 minutes.
    pub fn throughput_metrics(&self) -> Result<crate::models::ThroughputMetrics> {
        let one_hour_ago = (Utc::now() - chrono::Duration::hours(1)).to_rfc3339();

        // Tickets completed in the last hour
        let tickets_per_hour: f64 = self.conn.query_row(
            "SELECT COUNT(*) FROM events WHERE event_type = 'ticket_completed' AND timestamp >= ?1",
            params![one_hour_ago],
            |row| row.get::<_, i64>(0),
        ).unwrap_or(0) as f64;

        // Average tokens per completed ticket (all-time)
        let (total_tokens, completed_count): (i64, i64) = self.conn.query_row(
            "SELECT COALESCE(SUM(tokens_used), 0), COUNT(*) FROM events WHERE event_type = 'ticket_completed'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        ).unwrap_or((0, 0));
        let avg_tokens_per_ticket = if completed_count > 0 {
            total_tokens as f64 / completed_count as f64
        } else {
            0.0
        };

        // Merge conflict rate and timeout rate (last hour)
        let completions_hour: f64 = tickets_per_hour.max(1.0);
        let conflicts: i64 = self.conn.query_row(
            "SELECT COUNT(*) FROM events WHERE event_type = 'merge_conflict_requeued' AND timestamp >= ?1",
            params![one_hour_ago],
            |row| row.get(0),
        ).unwrap_or(0);
        let timeouts: i64 = self.conn.query_row(
            "SELECT COUNT(*) FROM events WHERE event_type = 'ticket_requeued_stale_in_progress' AND timestamp >= ?1",
            params![one_hour_ago],
            |row| row.get(0),
        ).unwrap_or(0);

        let pending_count: i64 = self.conn.query_row(
            "SELECT COUNT(*) FROM tickets WHERE status = 'pending'",
            [],
            |row| row.get(0),
        ).unwrap_or(0);

        Ok(crate::models::ThroughputMetrics {
            tickets_per_hour,
            avg_tokens_per_ticket,
            merge_conflict_rate: conflicts as f64 / completions_hour,
            timeout_rate: timeouts as f64 / completions_hour,
            pending_count,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn schema_version_is_current() {
        let db = Db::open_memory().unwrap();
        assert_eq!(db.schema_version().unwrap(), Db::CURRENT_SCHEMA_VERSION);
    }

    #[test]
    fn list_knowledge_by_domain_returns_only_matching_domain() {
        let db = Db::open_memory().unwrap();
        db.write_knowledge("learning", "t-001-success", r#"{"domain":"core"}"#).unwrap();
        db.write_knowledge("learning", "t-002-failure", r#"{"domain":"qa"}"#).unwrap();
        db.write_knowledge("core", "stack", "Rust").unwrap();

        let results = db.list_knowledge_by_domain("learning").unwrap();
        assert_eq!(results.len(), 2);
        assert!(results.iter().all(|e| e.domain == "learning"));

        let core_results = db.list_knowledge_by_domain("core").unwrap();
        assert_eq!(core_results.len(), 1);
        assert_eq!(core_results[0].key, "stack");

        let empty = db.list_knowledge_by_domain("nonexistent").unwrap();
        assert!(empty.is_empty());
    }

    #[test]
    fn schema_version_is_v5() {
        let db = Db::open_memory().unwrap();
        assert_eq!(db.schema_version().unwrap(), 5);
    }

    #[test]
    fn defer_count_starts_at_zero() {
        let db = Db::open_memory().unwrap();
        let id = db.create_ticket("T", "D", "core", 1).unwrap();
        let t = db.get_ticket(&id).unwrap().unwrap();
        assert_eq!(t.defer_count, 0);
    }

    #[test]
    fn increment_defer_count_increments_and_returns_new_value() {
        let db = Db::open_memory().unwrap();
        let id = db.create_ticket("T", "D", "core", 1).unwrap();
        let count1 = db.increment_defer_count(&id).unwrap();
        assert_eq!(count1, 1);
        let count2 = db.increment_defer_count(&id).unwrap();
        assert_eq!(count2, 2);
        let t = db.get_ticket(&id).unwrap().unwrap();
        assert_eq!(t.defer_count, 2);
    }

    #[test]
    fn claim_ticket_resets_defer_count() {
        let db = Db::open_memory().unwrap();
        let id = db.create_ticket("T", "D", "core", 1).unwrap();
        db.increment_defer_count(&id).unwrap();
        db.increment_defer_count(&id).unwrap();
        db.increment_defer_count(&id).unwrap();
        let t_before = db.get_ticket(&id).unwrap().unwrap();
        assert_eq!(t_before.defer_count, 3);

        db.register_agent("w-1", "worker", "general").unwrap();
        let claimed = db.claim_ticket_by_id(&id, "w-1").unwrap();
        assert!(claimed.is_some());
        let claimed = claimed.unwrap();
        assert_eq!(claimed.defer_count, 0);
    }

    #[test]
    fn list_tickets_with_defer_count_gt_filters_correctly() {
        let db = Db::open_memory().unwrap();
        let id1 = db.create_ticket("T1", "D", "core", 1).unwrap();
        let id2 = db.create_ticket("T2", "D", "core", 1).unwrap();
        let id3 = db.create_ticket("T3", "D", "core", 1).unwrap();
        db.increment_defer_count(&id1).unwrap(); // 1
        db.increment_defer_count(&id2).unwrap(); // 1
        db.increment_defer_count(&id2).unwrap(); // 2
        db.increment_defer_count(&id2).unwrap(); // 3
        db.increment_defer_count(&id2).unwrap(); // 4

        let stuck = db.list_tickets_with_defer_count_gt(0).unwrap();
        assert_eq!(stuck.len(), 2);

        let high = db.list_tickets_with_defer_count_gt(3).unwrap();
        assert_eq!(high.len(), 1);
        assert_eq!(high[0].id, id2);

        let none = db.list_tickets_with_defer_count_gt(10).unwrap();
        assert!(none.is_empty());

        // id3 not in any list (defer_count=0)
        let stuck_ids: Vec<&str> = stuck.iter().map(|t| t.id.as_str()).collect();
        assert!(!stuck_ids.contains(&id3.as_str()));
    }
}
