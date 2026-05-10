use std::fs::{File, OpenOptions};
use std::io::{self, BufReader};
use std::path::{Path, PathBuf};
use crate::core::format::BDBLogEntry;

pub struct WALManager {
    file: File,
    path: PathBuf,
}

impl WALManager {
    pub fn new(path: &Path) -> io::Result<Self> {
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .read(true)
            .open(path)?;

        Ok(Self {
            file,
            path: path.to_path_buf(),
        })
    }

    pub fn log(&mut self, entry: &mut BDBLogEntry) -> io::Result<()> {
        entry.write(&mut self.file)?;
        self.file.sync_all()?;
        Ok(())
    }

    pub fn read_all(&self) -> io::Result<Vec<BDBLogEntry>> {
        let file = File::open(&self.path)?;
        let mut reader = BufReader::new(file);
        let mut entries = Vec::new();

        while let Ok(entry) = BDBLogEntry::read(&mut reader) {
            entries.push(entry);
        }

        Ok(entries)
    }

    pub fn truncate(&mut self) -> io::Result<()> {
        self.file.set_len(0)?;
        self.file.sync_all()?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;
    use crate::core::format::EntryType;
    use std::io::Write;
    use std::fs::OpenOptions;

    #[test]
    fn test_wal_recovery_graceful_eof() {
        let dir = tempdir().unwrap();
        let wal_path = dir.path().join("test.wal");

        {
            let mut wal = WALManager::new(&wal_path).unwrap();
            let mut entry = BDBLogEntry::new(EntryType::Insert, b"key1".to_vec(), b"value1".to_vec());
            wal.log(&mut entry).unwrap();
        }

        // Append some garbage to simulate partial write / unexpected EOF
        {
            let mut file = OpenOptions::new().append(true).open(&wal_path).unwrap();
            file.write_all(b"partial data").unwrap();
        }

        let wal = WALManager::new(&wal_path).unwrap();
        let entries = wal.read_all().unwrap();

        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].key, b"key1");
    }
}
