use zawradb::core::lsm_tree::LSMTree;
use zawradb::core::format::TableType;
use zawradb::core::config::ZawraDBConfig;
use tempfile::tempdir;
use std::fs;
use std::io::Read;

#[test]
fn test_sstable_header_footer_presence() {
    let dir = tempdir().unwrap();
    let tree = LSMTree::new(dir.path(), TableType::LocalStore, 1024, ZawraDBConfig::default()).unwrap();

    // Insert some data and flush to create SSTable
    tree.put(b"key1".to_vec(), b"value1".to_vec()).unwrap();
    tree.flush().unwrap();

    // Find the SSTable file
    let entries = fs::read_dir(dir.path()).unwrap();
    let mut sst_path = None;
    for entry in entries {
        let path = entry.unwrap().path();
        if path.extension().map_or(false, |ext| ext == "sst") {
            sst_path = Some(path);
            break;
        }
    }

    let path = sst_path.expect("SSTable file not found");
    let mut file = fs::File::open(&path).unwrap();
    let mut buffer = Vec::new();
    file.read_to_end(&mut buffer).unwrap();

    // Check Header (First 7 bytes should be magic "ZAWRADB")
    assert_eq!(&buffer[0..7], b"ZAWRADB");

    // Check Footer (Last 48 bytes - we'll just check it's there and the file is large enough)
    assert!(buffer.len() >= 48 + 48); // Header + Footer minimum
}
