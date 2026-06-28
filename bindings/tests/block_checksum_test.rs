use browserdb::core::lsm_tree::LSMTree;
use browserdb::core::format::TableType;
use browserdb::core::config::BrowserDBConfig;
use tempfile::tempdir;
use std::fs::OpenOptions;
use std::io::{Write, Seek, SeekFrom};

#[test]
fn test_block_checksum_verification() {
    let dir = tempdir().unwrap();
    let path = dir.path();
    let mut config = BrowserDBConfig::default();
    config.lsm_tree.verify_checksums = true;

    let lsm_tree = LSMTree::new(path, TableType::History, 1024 * 1024, config).unwrap();

    let key = b"checksum_key".to_vec();
    let val = b"checksum_value".to_vec();
    lsm_tree.put(key.clone(), val.clone()).unwrap();
    lsm_tree.flush().unwrap();

    // 1. Verify it works normally
    assert!(lsm_tree.get(&key).is_some());
    
    // Release the memory map and file handle so Windows allows modification
    drop(lsm_tree);

    // 2. Corrupt a block in the SSTable
    for entry in std::fs::read_dir(path).unwrap() {
        let entry = entry.unwrap();
        if entry.path().extension().and_then(|s| s.to_str()) == Some("sst") {
            let mut file = OpenOptions::new().write(true).open(entry.path()).unwrap();
            // Seek to first data block (after header)
            file.seek(SeekFrom::Start(47)).unwrap();
            file.write_all(b"corrupted").unwrap();
        }
    }

    // Re-open the tree to clear any cached state if necessary,
    // though here we are testing the Mmap read which should hit the corrupted file.
    let mut config2 = BrowserDBConfig::default();
    config2.lsm_tree.verify_checksums = true;
    let lsm_tree2 = LSMTree::new(path, TableType::History, 1024 * 1024, config2).unwrap();

    // Should return None because checksum verification fails
    assert!(lsm_tree2.get(&key).is_none());
}
