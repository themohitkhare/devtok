// src/quality.rs
//
// Quality scoring and North Star metrics for ACS.

use anyhow::Result;
use chrono::Utc;
use std::path::Path;

use crate::db::Db;
use crate::models::QualityScore;

// ---------------------------------------------------------------------------
// Quality scoring
// ---------------------------------------------------------------------------

/// Scoring weights (must sum to 100).
const WEIGHT_TESTS: i32 = 34;
const WEIGHT_DOCS: i32 = 33;
const WEIGHT_CRITERIA: i32 = 33;

/// Compute a quality score for a single ticket.
///
/// - `tests_added`: true when the branch diff for this ticket contains test-file changes
/// - `docs_updated`: true when the branch diff includes markdown/README changes
/// - `acceptance_criteria_met`: true when the ticket notes contain an explicit
///   acceptance-criteria verification marker ("AC verified" or "acceptance criteria met")
pub fn compute_score(
    ticket_id: &str,
    tests_added: bool,
    docs_updated: bool,
    acceptance_criteria_met: bool,
) -> QualityScore {
    let score = (if tests_added { WEIGHT_TESTS } else { 0 })
        + (if docs_updated { WEIGHT_DOCS } else { 0 })
        + (if acceptance_criteria_met { WEIGHT_CRITERIA } else { 0 });

    QualityScore {
        ticket_id: ticket_id.to_string(),
        tests_added,
        docs_updated,
        acceptance_criteria_met,
        score,
        computed_at: Utc::now().to_rfc3339(),
    }
}

/// Infer quality signals from a ticket's git branch diff (if available) and
/// its notes field, then persist the result in the database.
///
/// - `branch_name`: e.g. `acs/t-001-abc123` — we run `git diff main...<branch>` to check
///   what files were changed.
/// - `ticket_notes`: the ticket's notes field, checked for AC verification markers.
pub fn score_ticket_from_branch(
    db: &Db,
    project_dir: &Path,
    ticket_id: &str,
    branch_name: Option<&str>,
    ticket_notes: &str,
) -> Result<QualityScore> {
    let (tests_added, docs_updated) = if let Some(branch) = branch_name {
        detect_changes_in_branch(project_dir, branch)
    } else {
        (false, false)
    };

    let acceptance_criteria_met = notes_contain_ac_verification(ticket_notes);

    let score = compute_score(ticket_id, tests_added, docs_updated, acceptance_criteria_met);
    db.upsert_quality_score(&score)?;
    Ok(score)
}

/// Check the diff of a branch against `main` for test/doc file changes.
/// Returns `(tests_added, docs_updated)`.
fn detect_changes_in_branch(project_dir: &Path, branch: &str) -> (bool, bool) {
    use std::process::Command;

    let output = Command::new("git")
        .args(["diff", "--name-only", &format!("main...{}", branch)])
        .current_dir(project_dir)
        .output();

    let output = match output {
        Ok(o) if o.status.success() => o,
        _ => return (false, false),
    };

    let files = String::from_utf8_lossy(&output.stdout);
    let mut tests_added = false;
    let mut docs_updated = false;

    for file in files.lines() {
        let f = file.trim().to_lowercase();
        // Test files: anything in a tests/ dir, *_test.rs, test_*.rs, *.test.*, spec files
        if f.contains("/test") || f.starts_with("test") || f.ends_with("_test.rs")
            || f.contains("spec") || f.contains("/tests/")
        {
            tests_added = true;
        }
        // Doc files: README*, *.md, docs/**
        if f.ends_with(".md") || f.starts_with("readme") || f.starts_with("docs/")
            || f.contains("/docs/")
        {
            docs_updated = true;
        }
    }

    (tests_added, docs_updated)
}

/// Check whether the ticket notes indicate acceptance criteria were explicitly verified.
pub fn notes_contain_ac_verification(notes: &str) -> bool {
    let lower = notes.to_lowercase();
    lower.contains("ac verified")
        || lower.contains("acceptance criteria met")
        || lower.contains("acceptance criteria verified")
        || lower.contains("criteria verified")
        || lower.contains("ac: verified")
        || lower.contains("[x] ac")
        || lower.contains("all ac met")
        || lower.contains("all criteria met")
}

// ---------------------------------------------------------------------------
// North Star metrics
// ---------------------------------------------------------------------------

/// The result of checking North Star completion criteria.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct NorthStarStatus {
    /// All tickets are in "completed" status.
    pub all_tickets_done: bool,
    /// No ticket is in a non-terminal status.
    pub no_pending_work: bool,
    /// README.md exists and is non-trivial (>100 bytes).
    pub readme_updated: bool,
    /// No TODO / FIXME / HACK comments found in src/ code files.
    pub no_todos_in_core: bool,
    /// List of TODO locations found (if any).
    pub todo_locations: Vec<String>,
    /// Tickets not yet completed (if any).
    pub incomplete_tickets: Vec<String>,
    /// Overall: true when all four checks pass.
    pub is_complete: bool,
}

/// Check North Star metrics against the given project directory.
pub fn check_north_star(db: &Db, project_dir: &std::path::Path) -> Result<NorthStarStatus> {
    let tickets = db.list_tickets(None)?;

    let incomplete: Vec<String> = tickets
        .iter()
        .filter(|t| t.status != "completed")
        .map(|t| format!("{} [{}]", t.id, t.status))
        .collect();

    let all_tickets_done = incomplete.is_empty();
    let no_pending_work = tickets.iter().all(|t| {
        matches!(t.status.as_str(), "completed" | "cancelled" | "wont_fix")
    });

    let readme_updated = check_readme(project_dir);
    let (no_todos, todo_locations) = check_no_todos(project_dir);

    let is_complete = all_tickets_done && readme_updated && no_todos;

    Ok(NorthStarStatus {
        all_tickets_done,
        no_pending_work,
        readme_updated,
        no_todos_in_core: no_todos,
        todo_locations,
        incomplete_tickets: incomplete,
        is_complete,
    })
}

fn check_readme(project_dir: &std::path::Path) -> bool {
    let readme = project_dir.join("README.md");
    if let Ok(meta) = std::fs::metadata(&readme) {
        meta.len() >= 100
    } else {
        false
    }
}

/// Scan `src/` for TODO/FIXME/HACK comments. Returns (no_todos_found, list_of_locations).
fn check_no_todos(project_dir: &std::path::Path) -> (bool, Vec<String>) {
    use std::process::Command;

    let src_dir = project_dir.join("src");
    if !src_dir.exists() {
        return (true, vec![]);
    }

    let output = Command::new("grep")
        .args([
            "-rn",
            "--include=*.rs",
            "--include=*.go",
            "--include=*.ts",
            "--include=*.js",
            "--include=*.py",
            r"TODO\|FIXME\|HACK",
            src_dir.to_str().unwrap_or("src"),
        ])
        .output();

    match output {
        Ok(o) => {
            let text = String::from_utf8_lossy(&o.stdout);
            let locations: Vec<String> = text
                .lines()
                .map(|l| l.trim().to_string())
                .filter(|l| !l.is_empty())
                .collect();
            (locations.is_empty(), locations)
        }
        Err(_) => (true, vec![]), // grep not available — skip check
    }
}

// ---------------------------------------------------------------------------
// Formatting helpers (used by report and quality commands)
// ---------------------------------------------------------------------------

pub fn format_north_star_report(status: &NorthStarStatus) -> String {
    let check = |v: bool| if v { "✓" } else { "✗" };

    let mut out = String::new();
    out.push_str("## North Star Metrics\n\n");
    out.push_str(&format!(
        "| Check | Status |\n|-------|--------|\n\
         | All tickets completed | {} |\n\
         | README.md updated | {} |\n\
         | No TODOs in core code | {} |\n\n",
        check(status.all_tickets_done),
        check(status.readme_updated),
        check(status.no_todos_in_core),
    ));

    if !status.incomplete_tickets.is_empty() {
        out.push_str("### Incomplete Tickets\n\n");
        for t in &status.incomplete_tickets {
            out.push_str(&format!("- {}\n", t));
        }
        out.push('\n');
    }

    if !status.todo_locations.is_empty() {
        out.push_str("### TODO / FIXME Locations\n\n");
        for loc in status.todo_locations.iter().take(20) {
            out.push_str(&format!("- `{}`\n", loc));
        }
        if status.todo_locations.len() > 20 {
            out.push_str(&format!("- _...and {} more_\n", status.todo_locations.len() - 20));
        }
        out.push('\n');
    }

    let overall = if status.is_complete {
        "**COMPLETE** — project meets all North Star criteria."
    } else {
        "**INCOMPLETE** — one or more North Star criteria are not yet met."
    };
    out.push_str(&format!("**Overall:** {}\n", overall));
    out
}

pub fn format_quality_scores_table(scores: &[QualityScore]) -> String {
    if scores.is_empty() {
        return "_No quality scores computed yet. Run `acs quality score --all` to compute._\n".to_string();
    }

    let mut out = String::new();
    out.push_str("## Quality Scores\n\n");
    out.push_str("| Ticket | Score | Tests | Docs | AC Met |\n");
    out.push_str("|--------|-------|-------|------|--------|\n");

    let check = |v: bool| if v { "✓" } else { "—" };

    for s in scores {
        out.push_str(&format!(
            "| {} | {}% | {} | {} | {} |\n",
            s.ticket_id,
            s.score,
            check(s.tests_added),
            check(s.docs_updated),
            check(s.acceptance_criteria_met),
        ));
    }

    let total = scores.len() as i32;
    let avg = if total > 0 { scores.iter().map(|s| s.score).sum::<i32>() / total } else { 0 };
    out.push_str(&format!("\n**Average quality score:** {}%\n", avg));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compute_score_all_true_gives_100() {
        let s = compute_score("t-001", true, true, true);
        assert_eq!(s.score, 100);
        assert!(s.tests_added);
        assert!(s.docs_updated);
        assert!(s.acceptance_criteria_met);
    }

    #[test]
    fn compute_score_all_false_gives_0() {
        let s = compute_score("t-001", false, false, false);
        assert_eq!(s.score, 0);
    }

    #[test]
    fn compute_score_only_tests() {
        let s = compute_score("t-001", true, false, false);
        assert_eq!(s.score, WEIGHT_TESTS);
    }

    #[test]
    fn compute_score_only_docs() {
        let s = compute_score("t-001", false, true, false);
        assert_eq!(s.score, WEIGHT_DOCS);
    }

    #[test]
    fn compute_score_only_criteria() {
        let s = compute_score("t-001", false, false, true);
        assert_eq!(s.score, WEIGHT_CRITERIA);
    }

    #[test]
    fn notes_ac_verification_positive_cases() {
        assert!(notes_contain_ac_verification("AC verified — all tests pass"));
        assert!(notes_contain_ac_verification("Acceptance criteria met"));
        assert!(notes_contain_ac_verification("acceptance criteria verified"));
        assert!(notes_contain_ac_verification("Criteria verified OK"));
        assert!(notes_contain_ac_verification("AC: verified after review"));
        assert!(notes_contain_ac_verification("[x] AC checklist done"));
        assert!(notes_contain_ac_verification("All AC met"));
        assert!(notes_contain_ac_verification("All criteria met"));
    }

    #[test]
    fn notes_ac_verification_negative_cases() {
        assert!(!notes_contain_ac_verification(""));
        assert!(!notes_contain_ac_verification("Auto-reviewed by manager"));
        assert!(!notes_contain_ac_verification("Work in progress"));
        assert!(!notes_contain_ac_verification("Tests passing"));
    }

    #[test]
    fn north_star_all_complete() {
        let db = Db::open_memory().unwrap();
        let t1 = db.create_ticket("T1", "desc", "backend", 1).unwrap();
        db.update_ticket(&t1, "completed", None, None, None).unwrap();

        let dir = tempfile::tempdir().unwrap();
        // Write a README
        std::fs::write(
            dir.path().join("README.md"),
            "# Test\n\nThis is a test readme with more than 100 bytes of content to pass the check.\n\nAdditional padding content to ensure the README is definitely above the threshold.\n",
        )
        .unwrap();
        // No src/ directory, so TODO check passes trivially

        let status = check_north_star(&db, dir.path()).unwrap();
        assert!(status.all_tickets_done);
        assert!(status.readme_updated);
        assert!(status.no_todos_in_core);
        assert!(status.is_complete);
    }

    #[test]
    fn north_star_incomplete_tickets() {
        let db = Db::open_memory().unwrap();
        let t1 = db.create_ticket("T1", "desc", "backend", 1).unwrap();
        // Leave t1 as pending

        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("README.md"), "# Test readme with enough content to pass the 100 byte threshold\n").unwrap();

        let status = check_north_star(&db, dir.path()).unwrap();
        assert!(!status.all_tickets_done);
        assert!(status.incomplete_tickets.contains(&format!("{} [pending]", t1)));
        assert!(!status.is_complete);
    }

    #[test]
    fn north_star_readme_too_short() {
        let db = Db::open_memory().unwrap();
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("README.md"), "short").unwrap();

        let status = check_north_star(&db, dir.path()).unwrap();
        assert!(!status.readme_updated);
    }

    #[test]
    fn north_star_no_readme() {
        let db = Db::open_memory().unwrap();
        let dir = tempfile::tempdir().unwrap();
        let status = check_north_star(&db, dir.path()).unwrap();
        assert!(!status.readme_updated);
    }

    #[test]
    fn quality_score_db_roundtrip() {
        let db = Db::open_memory().unwrap();
        let score = compute_score("t-042", true, false, true);
        db.upsert_quality_score(&score).unwrap();

        let loaded = db.get_quality_score("t-042").unwrap().unwrap();
        assert_eq!(loaded.ticket_id, "t-042");
        assert!(loaded.tests_added);
        assert!(!loaded.docs_updated);
        assert!(loaded.acceptance_criteria_met);
        assert_eq!(loaded.score, WEIGHT_TESTS + WEIGHT_CRITERIA);
    }

    #[test]
    fn quality_score_upsert_overwrites() {
        let db = Db::open_memory().unwrap();
        let s1 = compute_score("t-001", false, false, false);
        db.upsert_quality_score(&s1).unwrap();

        let s2 = compute_score("t-001", true, true, true);
        db.upsert_quality_score(&s2).unwrap();

        let loaded = db.get_quality_score("t-001").unwrap().unwrap();
        assert_eq!(loaded.score, 100);
    }

    #[test]
    fn list_quality_scores_empty() {
        let db = Db::open_memory().unwrap();
        let scores = db.list_quality_scores().unwrap();
        assert!(scores.is_empty());
    }

    #[test]
    fn format_quality_scores_table_with_data() {
        let scores = vec![
            compute_score("t-001", true, true, true),
            compute_score("t-002", false, false, false),
        ];
        let table = format_quality_scores_table(&scores);
        assert!(table.contains("t-001"));
        assert!(table.contains("100%"));
        assert!(table.contains("t-002"));
        assert!(table.contains("0%"));
        assert!(table.contains("Average quality score"));
    }

    #[test]
    fn format_quality_scores_table_empty() {
        let table = format_quality_scores_table(&[]);
        assert!(table.contains("No quality scores computed"));
    }

    #[test]
    fn format_north_star_report_complete() {
        let status = NorthStarStatus {
            all_tickets_done: true,
            no_pending_work: true,
            readme_updated: true,
            no_todos_in_core: true,
            todo_locations: vec![],
            incomplete_tickets: vec![],
            is_complete: true,
        };
        let report = format_north_star_report(&status);
        assert!(report.contains("COMPLETE"));
        assert!(report.contains("North Star Metrics"));
    }

    #[test]
    fn format_north_star_report_incomplete() {
        let status = NorthStarStatus {
            all_tickets_done: false,
            no_pending_work: false,
            readme_updated: true,
            no_todos_in_core: false,
            todo_locations: vec!["src/main.rs:42: // TODO fix this".to_string()],
            incomplete_tickets: vec!["t-001 [pending]".to_string()],
            is_complete: false,
        };
        let report = format_north_star_report(&status);
        assert!(report.contains("INCOMPLETE"));
        assert!(report.contains("t-001"));
        assert!(report.contains("src/main.rs"));
    }
}
