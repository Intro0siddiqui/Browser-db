use browserdb::{BrowserDB, HistoryEntry};
use browserdb::DatabaseMode;
use tempfile::tempdir;

fn now_ms() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis()
}

#[test]
fn test_hot_search_ranking_and_limit() {
    let dir = tempdir().unwrap();
    let db = BrowserDB::open(dir.path()).unwrap();

    let now = now_ms();
    let entries = vec![
        HistoryEntry {
            timestamp: now,
            url: "https://browserdb.example.com/a".to_string(),
            url_hash: 1,
            title: "BrowserDB alpha".to_string(),
            visit_count: 5,
        },
        HistoryEntry {
            timestamp: now - 1_000,
            url: "https://browserdb.example.com/b".to_string(),
            url_hash: 2,
            title: "BrowserDB beta".to_string(),
            visit_count: 50,
        },
        HistoryEntry {
            timestamp: now - 2_000,
            url: "https://other.example.com/c".to_string(),
            url_hash: 3,
            title: "Other site".to_string(),
            visit_count: 100,
        },
    ];

    for e in &entries {
        db.history().insert(e).unwrap();
    }

    // Case-insensitive substring match on url/title; "Other site" must be filtered out.
    let results = db.history().hot_search("browserdb", 10).unwrap();
    assert_eq!(results.len(), 2);
    // Higher visit_count wins regardless of slightly older timestamp.
    assert_eq!(results[0].url_hash, 2);
    assert_eq!(results[1].url_hash, 1);

    // Limit applies on top of the ranking.
    let top1 = db.history().hot_search("browserdb", 1).unwrap();
    assert_eq!(top1.len(), 1);
    assert_eq!(top1[0].url_hash, 2);
}

#[test]
fn test_hot_search_empty_query_returns_all_ranked() {
    let dir = tempdir().unwrap();
    let db = BrowserDB::open(dir.path()).unwrap();

    let now = now_ms();
    db.history().insert(&HistoryEntry {
        timestamp: now - 1_000,
        url: "https://a".to_string(),
        url_hash: 10,
        title: "A".to_string(),
        visit_count: 1,
    }).unwrap();
    db.history().insert(&HistoryEntry {
        timestamp: now,
        url: "https://b".to_string(),
        url_hash: 20,
        title: "B".to_string(),
        visit_count: 1,
    }).unwrap();

    let results = db.history().hot_search("", 10).unwrap();
    assert_eq!(results.len(), 2);
    // Same visit_count: tiebreak by recency (newer first).
    assert_eq!(results[0].url_hash, 20);
    assert_eq!(results[1].url_hash, 10);
}

#[test]
fn test_hot_search_ultra_mode() {
    let dir = tempdir().unwrap();
    let db = BrowserDB::open(dir.path()).unwrap();
    db.set_mode(DatabaseMode::Ultra).unwrap();

    let now = now_ms();
    db.history().insert(&HistoryEntry {
        timestamp: now,
        url: "https://browserdb.example/ultra".to_string(),
        url_hash: 99,
        title: "Ultra Test".to_string(),
        visit_count: 7,
    }).unwrap();

    let results = db.history().hot_search("browserdb", 10).unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].url_hash, 99);
}
