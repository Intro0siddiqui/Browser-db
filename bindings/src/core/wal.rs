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
