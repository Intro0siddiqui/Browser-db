use browserdb::core::lsm_tree::LSMTree;
use browserdb::core::format::TableType;
use browserdb::core::config::BrowserDBConfig;
use tempfile::tempdir;

#[test]
fn test_blob_separation() {
    let dir = tempdir().unwrap();
    let path = dir.path();
    let config = BrowserDBConfig::default();

    let lsm_tree = LSMTree::new(path, TableType::History, 1024 * 1024, config).unwrap();

    // 1. Small value (inline)
    let key1 = b"small_key".to_vec();
    let val1 = b"small_value".to_vec();
    lsm_tree.put(key1.clone(), val1.clone()).unwrap();

    let res1 = lsm_tree.get(&key1).unwrap();
    assert_eq!(res1.value, val1);
    // EntryType should be Insert (1)
    assert_eq!(res1.entry_type as u8, 1);

    // 2. Large value (blob)
    let key2 = b"large_key".to_vec();
    let val2 = vec![0u8; 100 * 1024]; // 100KB > 64KB
    lsm_tree.put(key2.clone(), val2.clone()).unwrap();

    let res2 = lsm_tree.get(&key2).unwrap();
    assert_eq!(res2.value, val2);
    // Even though it was stored as a blob, it should be transparently retrieved.

    // Check that it's actually stored as a BlobIndex (6) in the memtable/sst
    let shard = (key2.first().cloned().unwrap_or(0) % 16) as usize;
    let mem_entry = lsm_tree.inner.memtable[shard].read().get(&key2).unwrap();
    assert_eq!(mem_entry.entry_type as u8, 6); // BlobIndex
    assert_eq!(mem_entry.value.len(), 12); // BlobPointer size (8 + 4)
}

#[test]
fn test_blob_after_flush() {
    let dir = tempdir().unwrap();
    let path = dir.path();
    let config = BrowserDBConfig::default();

    let lsm_tree = LSMTree::new(path, TableType::History, 1024 * 1024, config).unwrap();

    let key = b"large_key_flush".to_vec();
    let val = vec![1u8; 100 * 1024];
    lsm_tree.put(key.clone(), val.clone()).unwrap();

    lsm_tree.flush().unwrap();

    let res = lsm_tree.get(&key).expect("Should find key after flush");
    assert_eq!(res.value, val);
}
