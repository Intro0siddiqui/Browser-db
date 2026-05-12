use browserdb::BrowserDB;
use browserdb::LocalStoreEntry;
use tempfile::tempdir;

#[test]
fn test_index_logic_structure() {
    let dir = tempdir().unwrap();
    let db = BrowserDB::open(dir.path().to_str().unwrap()).unwrap();

    let entry = LocalStoreEntry {
        origin_hash: 123,
        key: "test_key".to_string(),
        value: "indexed_val".to_string(),
    };

    // Insert with index
    db.localstore().insert_with_index(&entry, &["value"]).unwrap();

    // Query using value_eq which relies on the index
    let results = db.localstore().query()
        .value_eq("indexed_val".to_string())
        .execute()
        .unwrap();

    assert_eq!(results.len(), 1);
    assert_eq!(results[0].key, "test_key");
}
