use browserdb::*;
use tempfile::tempdir;

#[test]
fn test_batch_operations() {
    let temp_dir = tempdir().unwrap();
    let db_path = temp_dir.path().join("batch_test.bdb");

    let db = BrowserDB::open(db_path.to_str().unwrap()).expect("Failed to open database");

    let mut batch = db.new_batch();
    batch.set_setting("key1", "value1");
    batch.set_setting("key2", "value2");

    let entry = HistoryEntry {
        timestamp: 12345,
        url: "https://batch.com".to_string(),
        url_hash: 999,
        title: "Batch Title".to_string(),
        visit_count: 1,
    };
    batch.put_history(&entry).expect("Batch put history");

    batch.commit().expect("Batch commit");

    assert_eq!(db.settings().get("key1").unwrap(), Some("value1".to_string()));
    assert_eq!(db.settings().get("key2").unwrap(), Some("value2".to_string()));
    assert_eq!(db.history().get(999).unwrap().unwrap().title, "Batch Title");

    // Test delete in batch (Ultra mode)
    db.set_mode(DatabaseMode::Ultra).expect("Set Ultra");
    let mut batch = db.new_batch();
    batch.set_setting("key3", "value3");
    batch.delete_setting("key2");
    batch.commit().expect("Batch commit");

    assert_eq!(db.settings().get("key3").unwrap(), Some("value3".to_string()));
    assert_eq!(db.settings().get("key2").unwrap(), None);
}
