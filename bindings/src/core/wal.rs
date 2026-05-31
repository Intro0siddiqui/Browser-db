use std::fs::{File, OpenOptions};
use std::io::{self, BufReader, BufWriter, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::Duration;
use crate::core::format::BDBLogEntry;

pub struct WALManager {
    writer: Arc<Mutex<BufWriter<File>>>,
    path: PathBuf,
    stop_signal: Arc<AtomicBool>,
    flush_thread: Option<thread::JoinHandle<()>>,
}

impl WALManager {
    pub fn new(path: &Path) -> io::Result<Self> {
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .read(true)
            .open(path)?;

        let writer = Arc::new(Mutex::new(BufWriter::with_capacity(32 * 1024, file)));
        let stop_signal = Arc::new(AtomicBool::new(false));

        let writer_clone = Arc::clone(&writer);
        let stop_signal_clone = Arc::clone(&stop_signal);

        let flush_thread = thread::spawn(move || {
            while !stop_signal_clone.load(Ordering::Relaxed) {
                thread::sleep(Duration::from_millis(5));
                {
                    let mut w = writer_clone.lock().unwrap();
                    let _ = w.flush();
                    let _ = w.get_ref().sync_all();
                }
            }
            // Final flush
            let mut w = writer_clone.lock().unwrap();
            let _ = w.flush();
            let _ = w.get_ref().sync_all();
        });

        Ok(Self {
            writer,
            path: path.to_path_buf(),
            stop_signal,
            flush_thread: Some(flush_thread),
        })
    }

    pub fn log(&mut self, entry: &mut BDBLogEntry) -> io::Result<()> {
        let mut w = self.writer.lock().unwrap();
        entry.write(&mut *w)?;
        Ok(())
    }

    pub fn read_all(&self) -> io::Result<Vec<BDBLogEntry>> {
        // Ensure everything is flushed before reading
        {
            let mut w = self.writer.lock().unwrap();
            w.flush()?;
            w.get_ref().sync_all()?;
        }

        let file = File::open(&self.path)?;
        let mut reader = BufReader::new(file);
        let mut entries = Vec::new();

        while let Ok(entry) = BDBLogEntry::read(&mut reader) {
            entries.push(entry);
        }

        Ok(entries)
    }

    pub fn truncate(&mut self) -> io::Result<()> {
        let mut w = self.writer.lock().unwrap();
        w.flush()?;
        let file = w.get_mut();
        
        let mut attempts = 0;
        loop {
            match file.set_len(0) {
                Ok(_) => break,
                Err(e) if e.kind() == io::ErrorKind::PermissionDenied && attempts < 10 => {
                    attempts += 1;
                    std::thread::sleep(std::time::Duration::from_millis(50));
                }
                Err(e) => return Err(e),
            }
        }
        
        file.sync_all()?;
        Ok(())
    }
}

impl Drop for WALManager {
    fn drop(&mut self) {
        self.stop_signal.store(true, Ordering::Relaxed);
        if let Some(handle) = self.flush_thread.take() {
            let _ = handle.join();
        }
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
