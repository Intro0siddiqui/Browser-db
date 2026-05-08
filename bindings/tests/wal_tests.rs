use browserdb::*;
use tempfile::tempdir;
use std::fs;

#[test]
fn test_wal_recovery() {
    let temp_dir = tempdir().unwrap();
    let db_path = temp_dir.path().join("wal_recovery.bdb");

    {
        let db = BrowserDB::open(db_path.to_str().unwrap()).expect("Failed to create database");
        db.settings().set("key1", "value1").expect("Failed to set");
        // WAL is written, but MemTable is not flushed to SSTable yet.
    }

    // Re-open the database
    {
        let db = BrowserDB::open(db_path.to_str().unwrap()).expect("Failed to open database");
        let val = db.settings().get("key1").expect("Failed to get");
        assert_eq!(val, Some("value1".to_string()));
    }
}
