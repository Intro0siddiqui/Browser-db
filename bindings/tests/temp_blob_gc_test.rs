use browserdb::core::lsm_tree::LSMTree;
use browserdb::core::format::TableType;
use browserdb::core::config::BrowserDBConfig;
use tempfile::tempdir;
use std::fs;
use std::io;

#[test]
fn test_blob_garbage_collection() {
    let dir = tempdir().unwrap();
    let path = dir.path();
    let config = BrowserDBConfig::default();

    // 1. Initialize LSMTree
    let lsm_tree = LSMTree::new(path, TableType::History, 10 * 1024 * 1024, config).unwrap();

    // 2. Insert Key A with a 100KB value.
    let key_a = b"key_a".to_vec();
    let val_a1 = vec![1u8; 100 * 1024];
    lsm_tree.put(key_a.clone(), val_a1.clone()).unwrap();

    // 3. Insert Key B with a 100KB value.
    let key_b = b"key_b".to_vec();
    let val_b = vec![2u8; 100 * 1024];
    lsm_tree.put(key_b.clone(), val_b.clone()).unwrap();

    // 4. Overwrite Key A with a new 100KB value.
    let val_a2 = vec![3u8; 100 * 1024];
    lsm_tree.put(key_a.clone(), val_a2.clone()).unwrap();

    // 5. Assert that the physical blob_log file size is ~300KB.
    let blob_file_path = path.join("history.blob");
    let metadata = fs::metadata(&blob_file_path).unwrap();
    let size_before = metadata.len();
    // It should be at least 300KB.
    assert!(size_before >= 300 * 1024, "File size should be at least 300KB, got {}", size_before);

    // 5.1 Verify Iterator
    use browserdb::core::blob_log::BlobLogIterator;
    let iter = BlobLogIterator::new(&blob_file_path).unwrap();
    let entries: Vec<_> = iter.collect::<io::Result<Vec<_>>>().unwrap();
    assert_eq!(entries.len(), 3, "Should have 3 blob entries (A1, B, A2)");
    assert_eq!(entries[0].2, b"key_a");
    assert_eq!(entries[1].2, b"key_b");
    assert_eq!(entries[2].2, b"key_a");

    // 6. Trigger GC
    println!("Triggering GC...");
    lsm_tree.run_blob_gc().unwrap();
    println!("GC done.");

    // 7. Final Assertions
    let metadata_after = fs::metadata(&blob_file_path).unwrap();
    let size_after = metadata_after.len();
    assert!(size_after < size_before, "Size should shrink: {} < {}", size_after, size_before);
    // Should be around 200KB + small overhead for keys/headers
    assert!(size_after >= 200 * 1024 && size_after < 250 * 1024, "Unexpected size after GC: {}", size_after);

    let res_a = lsm_tree.get(&key_a).expect("Key A should exist");
    println!("Key A type: {:?}", res_a.entry_type);
    println!("Key A value len: {}", res_a.value.len());
    assert_eq!(res_a.value.len(), val_a2.len(), "Key A value length mismatch");
    assert!(res_a.value == val_a2, "Key A value content mismatch");

    let res_b = lsm_tree.get(&key_b).expect("Key B should exist");
    assert_eq!(res_b.value.len(), val_b.len(), "Key B value length mismatch");
    assert!(res_b.value == val_b, "Key B value content mismatch");

    // 8. Verify Transparent Scan
    let all_blobs = lsm_tree.scan_prefix(b"key_");
    assert_eq!(all_blobs.len(), 2, "Scan should return 2 entries");
    for entry in all_blobs {
        if entry.key == key_a {
            assert_eq!(entry.value.len(), val_a2.len(), "Scan Key A length mismatch");
            assert!(entry.value == val_a2, "Scan Key A content mismatch");
        } else if entry.key == key_b {
            assert_eq!(entry.value.len(), val_b.len(), "Scan Key B length mismatch");
            assert!(entry.value == val_b, "Scan Key B content mismatch");
        }
    }
    println!("Scan verification passed.");
}
