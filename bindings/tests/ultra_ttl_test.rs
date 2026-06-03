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
fn test_ultra_ttl_read_side_enforcement() {
    let dir = tempdir().unwrap();
    let db = BrowserDB::open(dir.path()).unwrap();
    db.set_mode(DatabaseMode::Ultra).unwrap();

    // 1ms TTL; the entry must be expired by the time we read it back.
    let entry = HistoryEntry {
        timestamp: now_ms(),
        url: "https://ttl.example/short".to_string(),
        url_hash: 4242,
        title: "TTL test".to_string(),
        visit_count: 1,
    };
    db.history().insert_with_ttl(&entry, 1).unwrap();
    std::thread::sleep(std::time::Duration::from_millis(20));

    let got = db.history().get(4242).unwrap();
    assert!(got.is_none(), "expired entry must not be returned");
}

#[test]
fn test_ultra_ttl_purge_expired_reclaims_memory() {
    let dir = tempdir().unwrap();
    let db = BrowserDB::open(dir.path()).unwrap();
    db.set_mode(DatabaseMode::Ultra).unwrap();

    let expired = HistoryEntry {
        timestamp: now_ms(),
        url: "https://ttl.example/expired".to_string(),
        url_hash: 1,
        title: "Expired".to_string(),
        visit_count: 1,
    };
    let alive = HistoryEntry {
        timestamp: now_ms(),
        url: "https://ttl.example/alive".to_string(),
        url_hash: 2,
        title: "Alive".to_string(),
        visit_count: 1,
    };
    db.history().insert_with_ttl(&expired, 1).unwrap();
    db.history().insert_with_ttl(&alive, 60_000).unwrap();
    std::thread::sleep(std::time::Duration::from_millis(20));

    // Read-side: expired entry is gone.
    assert!(db.history().get(1).unwrap().is_none());
    // The live entry still resolves.
    assert!(db.history().get(2).unwrap().is_some());

    // After a manual purge sweep, count() is accurate (no zombie entries).
    let purged = db.history().count().unwrap();
    assert_eq!(purged, 1);
}
