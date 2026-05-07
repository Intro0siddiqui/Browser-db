use browserdb::*;
use tempfile::tempdir;

#[test]
fn test_mode_migration() {
    let temp_dir = tempdir().unwrap();
    let db_path = temp_dir.path().join("migration_test.bdb");

    let db = BrowserDB::open(db_path.to_str().unwrap()).expect("Failed to create database");

    // 1. Insert data in Persistent Mode
    let entry = HistoryEntry {
        timestamp: 1000,
        url: "https://persistent.com".to_string(),
        url_hash: 1,
        title: "Persistent Page".to_string(),
        visit_count: 1,
    };
    db.history().insert(&entry).expect("Insert failed");

    // 2. Switch to Ultra Mode
    db.set_mode(DatabaseMode::Ultra).expect("Switch to Ultra failed");

    // 3. Verify data migrated to Ultra
    let retrieved = db.history().get(1).expect("Get failed");
    assert!(retrieved.is_some());
    assert_eq!(retrieved.unwrap().url, "https://persistent.com");

    // 4. Insert data in Ultra Mode
    let entry2 = HistoryEntry {
        timestamp: 2000,
        url: "https://ultra.com".to_string(),
        url_hash: 2,
        title: "Ultra Page".to_string(),
        visit_count: 2,
    };
    db.history().insert(&entry2).expect("Insert failed");

    // 5. Switch back to Persistent Mode
    db.set_mode(DatabaseMode::Persistent).expect("Switch to Persistent failed");

    // 6. Verify all data migrated back to Persistent
    let retrieved1 = db.history().get(1).expect("Get 1 failed");
    assert!(retrieved1.is_some());
    assert_eq!(retrieved1.unwrap().url, "https://persistent.com");

    let retrieved2 = db.history().get(2).expect("Get 2 failed");
    assert!(retrieved2.is_some());
    assert_eq!(retrieved2.unwrap().url, "https://ultra.com");
}
