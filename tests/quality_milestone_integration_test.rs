use std::sync::{Arc, Mutex};

use acs::config::Config;
use acs::db::Db;
use acs::quality::check_north_star;
use acs::quality::score_ticket_from_branch;

fn run_one_cycle(db: Arc<Mutex<Db>>, project_dir: std::path::PathBuf) {
    let config = Config::default_for("test");
    let rt = tokio::runtime::Runtime::new().expect("runtime");
    rt.block_on(async {
        let (tx, rx) = tokio::sync::watch::channel(false);
        tx.send(true).expect("send shutdown");
        acs::manager::run_loop(db, &config, project_dir, rx, false).await;
    });
}

#[test]
fn score_ticket_from_branch_persists_and_lists_quality_scores() {
    use std::fs;
    use std::process::Command;

    let db = Db::open_memory().expect("in-memory db");
    let repo = tempfile::tempdir().expect("temp repo");
    let repo_path = repo.path();

    Command::new("git")
        .args(["init"])
        .current_dir(repo_path)
        .output()
        .expect("git init");
    Command::new("git")
        .args(["config", "user.email", "test@example.com"])
        .current_dir(repo_path)
        .output()
        .expect("git user.email");
    Command::new("git")
        .args(["config", "user.name", "Test"])
        .current_dir(repo_path)
        .output()
        .expect("git user.name");
    Command::new("git")
        .args(["branch", "-M", "main"])
        .current_dir(repo_path)
        .output()
        .expect("git branch main");

    fs::write(
        repo_path.join("README.md"),
        "Base README content before branch changes.",
    )
    .expect("write readme");
    fs::create_dir_all(repo_path.join("src")).expect("mkdir src");
    fs::write(repo_path.join("src/lib.rs"), "pub fn base() {}\n").expect("write lib");

    Command::new("git")
        .args(["add", "."])
        .current_dir(repo_path)
        .output()
        .expect("git add");
    Command::new("git")
        .args(["commit", "-m", "init", "--no-gpg-sign"])
        .current_dir(repo_path)
        .output()
        .expect("git commit init");

    Command::new("git")
        .args(["checkout", "-b", "acs/t-042-test"])
        .current_dir(repo_path)
        .output()
        .expect("git checkout branch");

    fs::write(
        repo_path.join("README.md"),
        "Updated README with enough content to count as docs changes in quality scoring.",
    )
    .expect("write changed readme");
    fs::create_dir_all(repo_path.join("tests")).expect("mkdir tests");
    fs::write(
        repo_path.join("tests/quality_test.rs"),
        "#[test]\nfn quality_test() { assert!(true); }\n",
    )
    .expect("write quality test");

    Command::new("git")
        .args(["add", "."])
        .current_dir(repo_path)
        .output()
        .expect("git add branch");
    Command::new("git")
        .args(["commit", "-m", "ticket changes", "--no-gpg-sign"])
        .current_dir(repo_path)
        .output()
        .expect("git commit branch");

    let computed = score_ticket_from_branch(
        &db,
        repo_path,
        "t-042",
        Some("acs/t-042-test"),
        "AC verified after implementation",
    )
    .expect("score from branch");

    assert_eq!(computed.ticket_id, "t-042");
    assert_eq!(computed.score, 100);
    assert!(computed.tests_added);
    assert!(computed.docs_updated);
    assert!(computed.acceptance_criteria_met);

    let scores = db.list_quality_scores().expect("list quality scores");
    assert_eq!(scores.len(), 1);
    let stored = &scores[0];
    assert_eq!(stored.ticket_id, "t-042");
    assert_eq!(stored.score, 100);
    assert!(stored.tests_added);
    assert!(stored.docs_updated);
    assert!(stored.acceptance_criteria_met);
}

#[test]
fn check_north_star_uses_ticket_state_from_db() {
    let db = Db::open_memory().expect("in-memory db");
    let t1 = db.create_ticket("Done", "done", "backend", 1).expect("create t1");
    let t2 = db.create_ticket("Cancelled", "cancelled", "backend", 1).expect("create t2");
    let t3 = db.create_ticket("Pending", "pending", "backend", 1).expect("create t3");

    db.update_ticket(&t1, "completed", None, None, None)
        .expect("complete t1");
    db.update_ticket(&t2, "cancelled", None, None, None)
        .expect("cancel t2");
    db.update_ticket(&t3, "pending", None, None, None)
        .expect("leave t3 pending");

    let dir = tempfile::tempdir().expect("temp dir");
    std::fs::write(
        dir.path().join("README.md"),
        "# Project\n\nThis README is deliberately long enough to exceed one hundred bytes and satisfy north star README checks.",
    )
    .expect("write readme");

    let status = check_north_star(&db, dir.path()).expect("check north star");
    assert!(!status.all_tickets_done, "not all tickets are completed");
    assert!(
        !status.no_pending_work,
        "pending ticket means there is still pending work"
    );
    assert!(
        status.incomplete_tickets
            .iter()
            .any(|entry| entry == "t-002 [cancelled]"),
        "cancelled ticket should not count as completed"
    );
    assert!(
        status.incomplete_tickets
            .iter()
            .any(|entry| entry == "t-003 [pending]"),
        "pending ticket should be listed as incomplete"
    );
}

#[test]
fn milestone_auto_transitions_to_awaiting_approval_integration() {
    let db = Arc::new(Mutex::new(Db::open_memory().expect("in-memory db")));
    let milestone_id = {
        let guard = db.lock().expect("db lock");
        let milestone_id = guard
            .create_milestone("M1", "Ship all assigned tickets")
            .expect("create milestone");
        guard
            .update_milestone_status(milestone_id, "active")
            .expect("activate milestone");

        let t1 = guard
            .create_ticket("Ticket 1", "done", "core", 1)
            .expect("create t1");
        let t2 = guard
            .create_ticket("Ticket 2", "done", "core", 1)
            .expect("create t2");
        guard
            .update_ticket(&t1, "completed", None, None, None)
            .expect("complete t1");
        guard
            .update_ticket(&t2, "wont_fix", None, None, None)
            .expect("wont_fix t2");
        guard
            .assign_ticket_to_milestone(milestone_id, &t1)
            .expect("assign t1");
        guard
            .assign_ticket_to_milestone(milestone_id, &t2)
            .expect("assign t2");
        milestone_id
    };

    let project_dir = tempfile::tempdir().expect("project dir");
    run_one_cycle(db.clone(), project_dir.path().to_path_buf());

    let guard = db.lock().expect("db lock");
    let milestone = guard
        .get_milestone(milestone_id)
        .expect("get milestone")
        .expect("milestone exists");
    assert_eq!(milestone.status, "awaiting_approval");

    let ceo_message = guard.pop_inbox("ceo").expect("pop ceo inbox");
    assert!(ceo_message.is_some(), "ceo should receive review message");
    let ceo_message = ceo_message.expect("message exists");
    assert_eq!(ceo_message.msg_type, "milestone_ready_for_review");
}
