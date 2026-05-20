use std::fs::{self, OpenOptions};
use std::io::{self, Write, Seek};
use tempfile::tempdir;
use browserdb::core::lsm_tree::LSMTree;
use browserdb::core::format::TableType;
use browserdb::core::config::BrowserDBConfig;

#[test]
fn test_merge_stream_corrupt_sst() -> io::Result<()> {
    let dir = tempdir()?;
    let path = dir.path();

    let lsm_tree = LSMTree::new(path, TableType::History, 1024 * 1024, BrowserDBConfig::default())?;

    // Insert to Memtable
    lsm_tree.put(b"key1".to_vec(), b"val1".to_vec())?;
    lsm_tree.flush()?; // Create SSTable 1

    lsm_tree.put(b"key2".to_vec(), b"val2".to_vec())?;
    lsm_tree.flush()?; // Create SSTable 2

    // Corrupt the first SSTable's data area (starts at 47)
    for entry in fs::read_dir(path)? {
        let entry = entry?;
        if entry.path().extension().and_then(|s| s.to_str()) == Some("sst") {
            let mut file = OpenOptions::new().write(true).open(entry.path())?;
            file.set_len(100)?; // Truncate or mess up the file
            file.seek(std::io::SeekFrom::Start(50))?;
            file.write_all(b"garbage data to break read")?;
            break; // Corrupt just one
        }
    }

    // Attempt compaction. Since we are moving away from `unwrap()`, this should return `Err` rather than panicking.
    let l0_tables = lsm_tree.inner.levels[0].read().clone();
    let res = lsm_tree.merge_sstables(1, l0_tables);

    assert!(res.is_err()); // Ensure it gracefully errors out!

    Ok(())
}
