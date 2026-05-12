use browserdb::core::lsm_tree::LSMTree;
use browserdb::core::format::{EntryType, TableType, BDBLogEntry};
use browserdb::core::config::BrowserDBConfig;
use tempfile::tempdir;
use std::fs::OpenOptions;
use std::io::Write;

#[test]
fn test_batch_atomicity_on_recovery() {
    let dir = tempdir().unwrap();
    let wal_path = dir.path().join("localstore.wal");

    // Manually create a WAL with a complete batch and an incomplete batch
    {
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&wal_path)
            .unwrap();
        let mut writer = std::io::BufWriter::new(file);

        // 1. Complete batch
        BDBLogEntry::new(EntryType::BatchStart, vec![], vec![]).write(&mut writer).unwrap();
        BDBLogEntry::new(EntryType::Insert, b"key1".to_vec(), b"val1".to_vec()).write(&mut writer).unwrap();
        BDBLogEntry::new(EntryType::BatchEnd, vec![], vec![]).write(&mut writer).unwrap();

        // 2. Incomplete batch (missing BatchEnd)
        BDBLogEntry::new(EntryType::BatchStart, vec![], vec![]).write(&mut writer).unwrap();
        BDBLogEntry::new(EntryType::Insert, b"key2".to_vec(), b"val2".to_vec()).write(&mut writer).unwrap();

        writer.flush().unwrap();
    }

    // Open LSMTree and check recovery
    let tree = LSMTree::new(dir.path(), TableType::LocalStore, 1024 * 1024, BrowserDBConfig::default()).unwrap();

    assert!(tree.get(b"key1").is_some());
    assert!(tree.get(b"key2").is_none());
}
