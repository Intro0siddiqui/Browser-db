use browserdb::core::wal::WALManager;
use browserdb::core::format::{BDBLogEntry, EntryType};
use tempfile::tempdir;
use std::fs::OpenOptions;
use std::io::Write;

#[test]
fn test_wal_recovery_crash_sim() {
    let dir = tempdir().unwrap();
    let wal_path = dir.path().join("crash.wal");

    // Create a WAL and write a valid entry
    {
        let wal = WALManager::new(&wal_path).unwrap();
        let mut entry = BDBLogEntry::new(EntryType::Insert, b"key1".to_vec(), b"val1".to_vec());
        wal.log(&mut entry).unwrap();
    }

    // Now manually corrupt it with a partial write (simulating a crash)
    {
        let mut file = OpenOptions::new().append(true).open(&wal_path).unwrap();
        file.write_all(b"garbage that is not a full entry").unwrap();
    }

    // Try to recover
    let wal = WALManager::new(&wal_path).unwrap();
    let entries = wal.read_all(); // This should ideally NOT panic, but return Ok with valid entries up to the crash, or an Err.
    // Actually the current code uses unwraps in `LSMTree::new` which calls `wal.read_all()?` but if `read_all` fails it propagates.
    // We want to ensure it handles it gracefully. Currently `BDBLogEntry::read` might return unexpected EOF.
    assert!(entries.is_ok());
    let recovered = entries.unwrap();
    assert_eq!(recovered.len(), 1);
    assert_eq!(recovered[0].key, b"key1");
}
