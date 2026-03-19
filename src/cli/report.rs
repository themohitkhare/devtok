use anyhow::Result;
use std::collections::HashMap;
use std::fs;
use std::path::Path;

use crate::db::Db;

/// Generate a bootstrap summary report and write it to .acs/reports/bootstrap-summary.md.
/// Call this after `acs init --auto` or `acs init --spec` completes.
pub fn generate_bootstrap_report(acs_dir: &Path, db: &Db) -> Result<()> {
    let reports_dir = acs_dir.join("reports");
    fs::create_dir_all(&reports_dir)?;

    let content = build_bootstrap_report(db)?;
    let path = reports_dir.join("bootstrap-summary.md");
    fs::write(&path, content)?;
    eprintln!("[report] wrote {}", path.display());
    Ok(())
}

/// Generate a milestone report and write it to .acs/reports/milestone-N.md.
pub fn generate_milestone_report(acs_dir: &Path, db: &Db, milestone_num: usize) -> Result<()> {
    let reports_dir = acs_dir.join("reports");
    fs::create_dir_all(&reports_dir)?;

    let content = build_milestone_report(db, milestone_num)?;
    let path = reports_dir.join(format!("milestone-{}.md", milestone_num));
    fs::write(&path, content)?;
    eprintln!("[report] wrote {}", path.display());
    Ok(())
}

/// Generate a current progress report and write it to .acs/reports/progress.md.
/// This is the on-demand `acs report` command.
pub fn execute() -> Result<()> {
    let cwd = std::env::current_dir()?;
    let acs_dir = crate::cli::acs_dir::resolve_acs_dir(&cwd)?;
    let db = Db::open(&acs_dir.join("project.db"))?;

    let reports_dir = acs_dir.join("reports");
    fs::create_dir_all(&reports_dir)?;

    let content = build_progress_report(&db)?;
    let path = reports_dir.join("progress.md");
    fs::write(&path, &content)?;

    // Also print the report to stdout.
    println!("{}", content);
    eprintln!("[report] wrote {}", path.display());
    Ok(())
}

// ---------------------------------------------------------------------------
// Internal builders
// ---------------------------------------------------------------------------

fn build_bootstrap_report(db: &Db) -> Result<String> {
    let now = chrono::Utc::now().format("%Y-%m-%d %H:%M:%S UTC").to_string();
    let tickets = db.list_tickets(None)?;
    let kb_entries = db.list_all_knowledge()?;
    let total_tokens = db.total_tokens_used()?;

    let mut out = String::new();

    out.push_str("# Bootstrap Summary\n\n");
    out.push_str(&format!("_Generated: {}_\n\n", now));

    // --- Ticket summary ---
    out.push_str("## Tickets Created\n\n");
    out.push_str(&format!("**Total:** {}\n\n", tickets.len()));

    let mut by_domain: HashMap<String, Vec<_>> = HashMap::new();
    for t in &tickets {
        by_domain.entry(t.domain.clone()).or_default().push(t);
    }
    let mut domains: Vec<_> = by_domain.keys().collect();
    domains.sort();
    for domain in domains {
        let ts = &by_domain[domain];
        out.push_str(&format!("### {}\n\n", domain));
        for t in ts {
            out.push_str(&format!("- **{}** — {} _(priority: {})_\n", t.id, t.title, t.priority));
        }
        out.push('\n');
    }

    // --- KB entries ---
    out.push_str("## Knowledge Base Entries\n\n");
    if kb_entries.is_empty() {
        out.push_str("_No knowledge base entries yet._\n\n");
    } else {
        out.push_str(&format!("**Total:** {}\n\n", kb_entries.len()));
        for entry in &kb_entries {
            out.push_str(&format!("- `{}/{}` (v{}): {}\n",
                entry.domain, entry.key, entry.version,
                truncate(&entry.value, 120)));
        }
        out.push('\n');
    }

    // --- Architecture overview (from KB if present) ---
    if let Some(arch) = kb_entries.iter().find(|e| e.key == "architecture" || e.key == "overview") {
        out.push_str("## Architecture Overview\n\n");
        out.push_str(&arch.value);
        out.push_str("\n\n");
    }

    // --- Token usage ---
    if total_tokens > 0 {
        out.push_str("## Token Usage\n\n");
        out.push_str(&format!("**Total tokens used:** {}\n\n", total_tokens));
    }

    Ok(out)
}

fn build_milestone_report(db: &Db, milestone_num: usize) -> Result<String> {
    let now = chrono::Utc::now().format("%Y-%m-%d %H:%M:%S UTC").to_string();
    let all_tickets = db.list_tickets(None)?;
    let completed = all_tickets.iter().filter(|t| t.status == "completed").collect::<Vec<_>>();
    let agents = db.list_agents()?;
    let total_tokens = db.total_tokens_used()?;
    let events = db.recent_events(50)?;

    let mut out = String::new();

    out.push_str(&format!("# Milestone {} Report\n\n", milestone_num));
    out.push_str(&format!("_Generated: {}_\n\n", now));

    // --- What was built ---
    out.push_str("## Completed Tickets\n\n");
    if completed.is_empty() {
        out.push_str("_No tickets completed yet._\n\n");
    } else {
        out.push_str(&format!("**{} tickets completed:**\n\n", completed.len()));
        for t in &completed {
            let assignee = t.assignee.as_deref().unwrap_or("unassigned");
            out.push_str(&format!("- **{}** — {} _(domain: {}, assignee: {})_\n",
                t.id, t.title, t.domain, assignee));
        }
        out.push('\n');
    }

    // --- Ticket status summary ---
    out.push_str("## Ticket Status Summary\n\n");
    let counts = db.count_by_status()?;
    let total: i64 = counts.iter().map(|(_, c)| c).sum();
    out.push_str(&format!("| Status | Count |\n|--------|-------|\n"));
    for (status, count) in &counts {
        out.push_str(&format!("| {} | {} |\n", status, count));
    }
    out.push_str(&format!("| **Total** | **{}** |\n\n", total));

    // --- Branches merged (from git branches) ---
    out.push_str("## Git Branch Status\n\n");
    match git_branch_summary() {
        Ok(branch_info) => {
            out.push_str(&branch_info);
            out.push('\n');
        }
        Err(_) => {
            out.push_str("_Unable to read git branch information._\n\n");
        }
    }

    // --- Agent activity ---
    out.push_str("## Agent Activity\n\n");
    if agents.is_empty() {
        out.push_str("_No agents currently registered._\n\n");
    } else {
        for a in &agents {
            let ticket_info = a.current_ticket.as_deref().unwrap_or("-");
            out.push_str(&format!("- **{}** ({}/{}) — {} [{}]\n",
                a.id, a.role, a.persona, a.status, ticket_info));
        }
        out.push('\n');
    }

    // --- Recent events / decisions ---
    out.push_str("## Recent Events\n\n");
    if events.is_empty() {
        out.push_str("_No events recorded._\n\n");
    } else {
        for e in events.iter().take(20) {
            let agent = e.agent.as_deref().unwrap_or("system");
            let tokens = e.tokens_used.unwrap_or(0);
            let token_str = if tokens > 0 { format!(" (+{} tokens)", tokens) } else { String::new() };
            out.push_str(&format!("- `{}` **{}** [{}]: {}{}\n",
                &e.timestamp[..19], e.event_type, agent,
                truncate(&e.detail, 100), token_str));
        }
        out.push('\n');
    }

    // --- Decisions (ADRs from KB) ---
    let kb_entries = db.list_all_knowledge()?;
    let adrs: Vec<_> = kb_entries.iter()
        .filter(|e| e.key.starts_with("adr") || e.key.starts_with("decision"))
        .collect();
    if !adrs.is_empty() {
        out.push_str("## Decisions Made (ADRs)\n\n");
        for adr in adrs {
            out.push_str(&format!("### `{}/{}`\n\n{}\n\n", adr.domain, adr.key, adr.value));
        }
    }

    // --- Token usage ---
    out.push_str("## Token Usage\n\n");
    out.push_str(&format!("**Total tokens used:** {}\n\n", total_tokens));

    Ok(out)
}

fn build_progress_report(db: &Db) -> Result<String> {
    let now = chrono::Utc::now().format("%Y-%m-%d %H:%M:%S UTC").to_string();
    let all_tickets = db.list_tickets(None)?;
    let agents = db.list_agents()?;
    let kb_entries = db.list_all_knowledge()?;
    let events = db.recent_events(30)?;
    let total_tokens = db.total_tokens_used()?;

    let mut out = String::new();

    out.push_str("# Project Progress Report\n\n");
    out.push_str(&format!("_Generated: {}_\n\n", now));

    // --- Ticket status summary ---
    out.push_str("## Ticket Status Summary\n\n");
    let counts = db.count_by_status()?;
    let total: i64 = counts.iter().map(|(_, c)| c).sum();
    out.push_str("| Status | Count |\n|--------|-------|\n");
    let status_order = ["completed", "in_progress", "review_pending", "pending", "blocked"];
    let count_map: HashMap<String, i64> = counts.into_iter().collect();
    for s in &status_order {
        if let Some(c) = count_map.get(*s) {
            out.push_str(&format!("| {} | {} |\n", s, c));
        }
    }
    // Any other statuses
    for (s, c) in &count_map {
        if !status_order.contains(&s.as_str()) {
            out.push_str(&format!("| {} | {} |\n", s, c));
        }
    }
    out.push_str(&format!("| **Total** | **{}** |\n\n", total));

    // --- All tickets table ---
    out.push_str("## All Tickets\n\n");
    out.push_str("| ID | Title | Domain | Status | Assignee |\n");
    out.push_str("|----|-------|--------|--------|----------|\n");
    for t in &all_tickets {
        let assignee = t.assignee.as_deref().unwrap_or("-");
        out.push_str(&format!("| {} | {} | {} | {} | {} |\n",
            t.id, t.title, t.domain, t.status, assignee));
    }
    out.push('\n');

    // --- Agent activity ---
    out.push_str("## Agent Activity\n\n");
    if agents.is_empty() {
        out.push_str("_No agents currently registered._\n\n");
    } else {
        out.push_str("| ID | Role | Persona | Status | Current Ticket |\n");
        out.push_str("|----|------|---------|--------|----------------|\n");
        for a in &agents {
            let ticket_info = a.current_ticket.as_deref().unwrap_or("-");
            out.push_str(&format!("| {} | {} | {} | {} | {} |\n",
                a.id, a.role, a.persona, a.status, ticket_info));
        }
        out.push('\n');
    }

    // --- Decisions made (ADRs from KB) ---
    let adrs: Vec<_> = kb_entries.iter()
        .filter(|e| e.key.starts_with("adr") || e.key.starts_with("decision"))
        .collect();
    if !adrs.is_empty() {
        out.push_str("## Decisions Made (ADRs)\n\n");
        for adr in adrs {
            out.push_str(&format!("### `{}/{}`\n\n{}\n\n", adr.domain, adr.key, adr.value));
        }
    }

    // --- Token usage ---
    out.push_str("## Token Usage\n\n");
    out.push_str(&format!("**Total tokens used:** {}\n\n", total_tokens));

    // --- Git branch status ---
    out.push_str("## Git Branch Status\n\n");
    match git_branch_summary() {
        Ok(info) => { out.push_str(&info); out.push('\n'); }
        Err(_) => out.push_str("_Unable to read git branch information._\n\n"),
    }

    // --- Recent events ---
    out.push_str("## Recent Events\n\n");
    if events.is_empty() {
        out.push_str("_No events recorded._\n\n");
    } else {
        for e in &events {
            let agent = e.agent.as_deref().unwrap_or("system");
            let tokens = e.tokens_used.unwrap_or(0);
            let token_str = if tokens > 0 { format!(" (+{} tokens)", tokens) } else { String::new() };
            out.push_str(&format!("- `{}` **{}** [{}]: {}{}\n",
                &e.timestamp[..19], e.event_type, agent,
                truncate(&e.detail, 100), token_str));
        }
        out.push('\n');
    }

    Ok(out)
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        format!("{}…", &s[..s.char_indices().nth(max).map(|(i, _)| i).unwrap_or(s.len())])
    }
}

fn git_branch_summary() -> Result<String> {
    use std::process::Command;

    let output = Command::new("git")
        .args(["branch", "-a", "--format=%(refname:short) %(objectname:short)"])
        .output()?;

    if !output.status.success() {
        anyhow::bail!("git branch failed");
    }

    let branch_list = String::from_utf8_lossy(&output.stdout);
    let acs_branches: Vec<&str> = branch_list
        .lines()
        .filter(|l| l.contains("acs/"))
        .collect();

    let current_output = Command::new("git")
        .args(["rev-parse", "--abbrev-ref", "HEAD"])
        .output()?;
    let current = String::from_utf8_lossy(&current_output.stdout).trim().to_string();

    let mut out = String::new();
    out.push_str(&format!("**Current branch:** `{}`\n\n", current));

    if acs_branches.is_empty() {
        out.push_str("_No `acs/*` branches found._\n");
    } else {
        out.push_str(&format!("**ACS branches ({}):**\n\n", acs_branches.len()));
        for b in acs_branches.iter().take(20) {
            out.push_str(&format!("- `{}`\n", b.trim()));
        }
        if acs_branches.len() > 20 {
            out.push_str(&format!("- _...and {} more_\n", acs_branches.len() - 20));
        }
    }

    Ok(out)
}
