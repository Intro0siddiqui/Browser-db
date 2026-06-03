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

#[test]
fn test_insert_with_index_filters_by_field() {
    let dir = tempdir().unwrap();
    let db = BrowserDB::open(dir.path().to_str().unwrap()).unwrap();

    // Only the `key` index should be built; the `value` index must be absent.
    let entry = LocalStoreEntry {
        origin_hash: 9,
        key: "k_only".to_string(),
        value: "v".to_string(),
    };
    db.localstore().insert_with_index(&entry, &["key"]).unwrap();

    let by_value = db.localstore().query()
        .value_eq("v".to_string())
        .execute()
        .unwrap();
    assert!(by_value.is_empty(), "value index must not be built when only 'key' is requested");

    // Inserting again with the default (value) index should make the entry
    // discoverable through value_eq.
    let entry2 = LocalStoreEntry {
        origin_hash: 9,
        key: "k_default".to_string(),
        value: "v".to_string(),
    };
    db.localstore().insert_with_index(&entry2, &[]).unwrap();

    let by_value = db.localstore().query()
        .value_eq("v".to_string())
        .execute()
        .unwrap();
    assert_eq!(by_value.len(), 1);
    assert_eq!(by_value[0].key, "k_default");
}

#[test]
fn test_insert_with_index_unknown_field_errors() {
    let dir = tempdir().unwrap();
    let db = BrowserDB::open(dir.path().to_str().unwrap()).unwrap();

    let entry = LocalStoreEntry {
        origin_hash: 1,
        key: "k".to_string(),
        value: "v".to_string(),
    };
    let err = db.localstore().insert_with_index(&entry, &["bogus_field"]).unwrap_err();
    let msg = err.to_string();
    assert!(msg.contains("bogus_field"), "error should mention the bad field name, got: {}", msg);
}
