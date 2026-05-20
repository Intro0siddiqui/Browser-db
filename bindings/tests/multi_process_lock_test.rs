use browserdb::BrowserDB;
use tempfile::tempdir;

#[test]
fn test_multi_process_locking() {
    let dir = tempdir().unwrap();
    let path = dir.path();

    // First process opens the DB
    let _db1 = BrowserDB::open(path).expect("Failed to open DB first time");

    // Second process tries to open the same DB - should fail
    let db2_result = BrowserDB::open(path);

    assert!(db2_result.is_err());
    let err_msg = db2_result.err().unwrap().to_string();
    assert!(err_msg.contains("already in use") || err_msg.contains("locked") || err_msg.contains("Resource temporarily unavailable"));
}

#[test]
fn test_lock_release_on_drop() {
    let dir = tempdir().unwrap();
    let path = dir.path();

    {
        let _db1 = BrowserDB::open(path).expect("Failed to open DB");
    } // _db1 dropped here, lock should be released

    let db2_result = BrowserDB::open(path);
    assert!(db2_result.is_ok(), "Lock should have been released");
}
