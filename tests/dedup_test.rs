use acs::db::Db;

#[test]
fn test_extract_keywords_basic() {
    let kws = Db::extract_keywords("Add user authentication system");
    assert!(kws.contains("add"));
    assert!(kws.contains("user"));
    assert!(kws.contains("authentication"));
    assert!(kws.contains("system"));
    // "the", "a" etc. are stop words and should not appear
    assert!(!kws.contains("a"));
}

#[test]
fn test_extract_keywords_filters_short_words() {
    let kws = Db::extract_keywords("go to db on it");
    // All words here are <= 2 chars or stop words
    assert!(kws.is_empty());
}

#[test]
fn test_extract_keywords_deduplicates() {
    let kws = Db::extract_keywords("auth auth AUTH Auth");
    assert_eq!(kws.len(), 1);
    assert!(kws.contains("auth"));
}

#[test]
fn test_store_and_find_similar_exact_match() {
    let db = Db::open_memory().unwrap();
    let id = db.create_ticket("Add user authentication", "Implement login and signup", "backend", 1).unwrap();
    db.store_ticket_keywords(&id, "Add user authentication", "Implement login and signup").unwrap();

    // Search with identical text should return high similarity
    let similar = db.find_similar_tickets("Add user authentication", "Implement login and signup").unwrap();
    assert!(!similar.is_empty());
    assert_eq!(similar[0].0, id);
    assert!((similar[0].2 - 1.0).abs() < f64::EPSILON, "Exact match should be 1.0, got {}", similar[0].2);
}

#[test]
fn test_find_similar_partial_overlap() {
    let db = Db::open_memory().unwrap();
    let id = db.create_ticket("Add user authentication system", "Login and signup flow", "backend", 1).unwrap();
    db.store_ticket_keywords(&id, "Add user authentication system", "Login and signup flow").unwrap();

    // Partial overlap
    let similar = db.find_similar_tickets("Fix user authentication bug", "Authentication not working").unwrap();
    assert!(!similar.is_empty());
    let score = similar[0].2;
    assert!(score > 0.0, "Should have some similarity");
    assert!(score < 1.0, "Should not be exact match");
}

#[test]
fn test_find_similar_no_match() {
    let db = Db::open_memory().unwrap();
    let id = db.create_ticket("Add user authentication", "Login flow", "backend", 1).unwrap();
    db.store_ticket_keywords(&id, "Add user authentication", "Login flow").unwrap();

    let similar = db.find_similar_tickets("Deploy infrastructure monitoring", "Set up Grafana dashboards").unwrap();
    assert!(similar.is_empty());
}

#[test]
fn test_find_similar_multiple_tickets_sorted_by_score() {
    let db = Db::open_memory().unwrap();

    let id1 = db.create_ticket("Add authentication system", "User login", "backend", 1).unwrap();
    db.store_ticket_keywords(&id1, "Add authentication system", "User login").unwrap();

    let id2 = db.create_ticket("Deploy monitoring dashboard", "Grafana setup", "devops", 2).unwrap();
    db.store_ticket_keywords(&id2, "Deploy monitoring dashboard", "Grafana setup").unwrap();

    let id3 = db.create_ticket("Fix authentication bug", "Login not working for users", "backend", 1).unwrap();
    db.store_ticket_keywords(&id3, "Fix authentication bug", "Login not working for users").unwrap();

    // Search for auth-related ticket
    let similar = db.find_similar_tickets("Update authentication middleware", "User session handling").unwrap();
    assert!(similar.len() >= 2, "Should match at least 2 auth tickets");
    // Results should be sorted by score descending
    for i in 1..similar.len() {
        assert!(similar[i - 1].2 >= similar[i].2, "Results should be sorted by score descending");
    }
}

#[test]
fn test_find_similar_empty_input() {
    let db = Db::open_memory().unwrap();
    let id = db.create_ticket("Add authentication", "Login", "backend", 1).unwrap();
    db.store_ticket_keywords(&id, "Add authentication", "Login").unwrap();

    // Empty input should return empty results
    let similar = db.find_similar_tickets("", "").unwrap();
    assert!(similar.is_empty());
}

#[test]
fn test_dedup_with_force_flag_cli() {
    // Test at the DB level: creating duplicate tickets with force should work
    // (force bypasses the check at CLI level, so DB always allows creation)
    let db = Db::open_memory().unwrap();
    let id1 = db.create_ticket("Add auth system", "Login flow", "backend", 1).unwrap();
    db.store_ticket_keywords(&id1, "Add auth system", "Login flow").unwrap();

    // Creating identical ticket at DB level always succeeds
    let id2 = db.create_ticket("Add auth system", "Login flow", "backend", 1).unwrap();
    db.store_ticket_keywords(&id2, "Add auth system", "Login flow").unwrap();

    assert_ne!(id1, id2);
    let tickets = db.list_tickets(None).unwrap();
    assert_eq!(tickets.len(), 2);
}

#[test]
fn test_jaccard_similarity_threshold() {
    let db = Db::open_memory().unwrap();

    // Create a ticket with specific keywords
    let id = db.create_ticket(
        "Implement user registration endpoint",
        "Create REST API for user signup with email validation",
        "backend", 1
    ).unwrap();
    db.store_ticket_keywords(
        &id,
        "Implement user registration endpoint",
        "Create REST API for user signup with email validation",
    ).unwrap();

    // Very similar ticket should score > 0.7
    let similar = db.find_similar_tickets(
        "Implement user registration API",
        "Create REST endpoint for user signup with email",
    ).unwrap();
    assert!(!similar.is_empty());
    assert!(similar[0].2 >= 0.70, "Very similar ticket should score >= 70%, got {}%", (similar[0].2 * 100.0).round());

    // Completely different ticket should not appear
    let similar2 = db.find_similar_tickets(
        "Configure database backup schedule",
        "Set up automated PostgreSQL backups",
    ).unwrap();
    assert!(similar2.is_empty() || similar2[0].2 < 0.70);
}
