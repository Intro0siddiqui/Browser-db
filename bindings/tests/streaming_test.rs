use browserdb::core::lsm_tree::LSMTree;
use browserdb::core::format::TableType;
use browserdb::core::config::BrowserDBConfig;
use tempfile::tempdir;

#[test]
fn test_streaming_iter_memtable() {
    let dir = tempdir().unwrap();
    let config = BrowserDBConfig::default();
    let tree = LSMTree::new(dir.path(), TableType::History, 1024 * 1024, config).unwrap();

    let key1 = b"apple".to_vec();
    let val1 = b"red".to_vec();
    let key2 = b"banana".to_vec();
    let val2 = b"yellow".to_vec();
    let key3 = b"cherry".to_vec();
    let val3 = b"red".to_vec();

    tree.put(key1.clone(), val1.clone()).unwrap();
    tree.put(key2.clone(), val2.clone()).unwrap();
    tree.put(key3.clone(), val3.clone()).unwrap();

    // Test streaming iter with prefix
    let prefix = b"a";
    let mut iter = tree.streaming_iter(prefix);
    let entry = iter.next().unwrap().unwrap();
    assert_eq!(entry.key, key1);
    assert_eq!(entry.value, val1);
    assert!(iter.next().is_none());

    // Test streaming iter without prefix
    let mut iter = tree.streaming_iter(b"");
    let mut keys = Vec::new();
    while let Some(Ok(entry)) = iter.next() {
        keys.push(entry.key);
    }
    keys.sort();
    assert_eq!(keys.len(), 3);
    assert!(keys.contains(&key1));
    assert!(keys.contains(&key2));
    assert!(keys.contains(&key3));
}
