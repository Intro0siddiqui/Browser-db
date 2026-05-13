use std::sync::Arc;
use std::thread;
use std::time::Duration;
use tempfile::tempdir;
use browserdb::core::lsm_tree::LSMTree;
use browserdb::core::format::TableType;
use browserdb::core::config::BrowserDBConfig;

#[test]
fn test_compaction_cascade() {
    let dir = tempdir().unwrap();
    let path = dir.path();

    let mut config = BrowserDBConfig::default();
    // Set low thresholds to trigger cascading compaction easily
    config.lsm_tree.max_level0_files = 2;
    config.lsm_tree.level_size_thresholds_mb = vec![1, 2, 4]; // L1: 1MB, L2: 2MB, L3: 4MB

    let lsm_tree = LSMTree::new(path, TableType::History, 1024 * 1024, config).unwrap();

    // 1. Fill L0 to trigger L0 -> L1
    for i in 0..100 {
        let key = format!("key{:03}", i).into_bytes();
        let value = vec![0u8; 10 * 1024]; // 10KB
        lsm_tree.put(key, value).unwrap();
        lsm_tree.flush().unwrap();
    }

    // Wait for background compaction with retries
    let mut found_high_level = false;
    for _ in 0..10 {
        thread::sleep(Duration::from_millis(500));
        for level in 1..10 {
            if !lsm_tree.inner.levels[level].read().is_empty() {
                found_high_level = true;
                println!("Found files at level {}", level);
                break;
            }
        }
        if found_high_level { break; }
    }
    assert!(found_high_level, "Should have compacted to L1 or higher");

    // 2. Keep filling to trigger L1 -> L2
    // We need more than 1MB in L1 to trigger L1 -> L2
    for i in 100..300 {
        let key = format!("key{:03}", i).into_bytes();
        let value = vec![0u8; 10 * 1024]; // 10KB
        lsm_tree.put(key, value).unwrap();
        lsm_tree.flush().unwrap();
    }

    let mut found_l2 = false;
    for _ in 0..10 {
        thread::sleep(Duration::from_millis(500));
        for level in 2..10 {
            if !lsm_tree.inner.levels[level].read().is_empty() {
                found_l2 = true;
                println!("Found files at level {}", level);
                break;
            }
        }
        if found_l2 { break; }
    }
    // Note: Due to multi-threaded nature, we might need more data or more time,
    // but the logic should trigger eventually.
    // L1 threshold is 1MB. 300 entries of 10KB is ~3MB, which should definitely overflow L1.
    assert!(found_l2, "Should have compacted to L2 or higher");
}
