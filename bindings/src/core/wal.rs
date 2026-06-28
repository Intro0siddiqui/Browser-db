use std::fs::{File, OpenOptions};
use std::io::{self, BufReader, BufWriter, Write, Seek};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::Duration;
use crossbeam_channel::{bounded, Sender, Receiver};
use crate::core::format::BDBLogEntry;

const WAL_CHANNEL_CAPACITY: usize = 4096;

pub struct WALManager {
    sender: Sender<Vec<u8>>,
    writer: Arc<Mutex<BufWriter<File>>>,
    path: PathBuf,
    stop_signal: Arc<AtomicBool>,
    writer_thread: Option<thread::JoinHandle<()>>,
    flush_thread: Option<thread::JoinHandle<()>>,
}

impl WALManager {
    pub fn new(path: &Path) -> io::Result<Self> {
        let mut file = OpenOptions::new()
            .create(true)
            .write(true)
            .read(true)
            .open(path)?;
            
        file.seek(std::io::SeekFrom::End(0))?;

        let writer = Arc::new(Mutex::new(BufWriter::with_capacity(32 * 1024, file)));
        let stop_signal = Arc::new(AtomicBool::new(false));

        let (sender, receiver): (Sender<Vec<u8>>, crossbeam_channel::Receiver<Vec<u8>>) = bounded(WAL_CHANNEL_CAPACITY);

        let writer_clone = Arc::clone(&writer);
        let stop_clone = Arc::clone(&stop_signal);

        let writer_thread = thread::spawn(move || {
            while !stop_clone.load(Ordering::Relaxed) {
                match receiver.recv_timeout(Duration::from_millis(1)) {
                    Ok(bytes) => {
                        let mut w = writer_clone.lock().unwrap();
                        let _ = w.write_all(&bytes);
                    }
                    Err(crossbeam_channel::RecvTimeoutError::Timeout) => {}
                    Err(crossbeam_channel::RecvTimeoutError::Disconnected) => break,
                }
            }
            while let Ok(bytes) = receiver.try_recv() {
                let mut w = writer_clone.lock().unwrap();
                let _ = w.write_all(&bytes);
            }
        });

        let flush_writer = Arc::clone(&writer);
        let flush_stop = Arc::clone(&stop_signal);
        let flush_thread = thread::spawn(move || {
            while !flush_stop.load(Ordering::Relaxed) {
                thread::sleep(Duration::from_millis(5));
                let mut w = flush_writer.lock().unwrap();
                let _ = w.flush();
                let _ = w.get_ref().sync_all();
            }
            let mut w = flush_writer.lock().unwrap();
            let _ = w.flush();
            let _ = w.get_ref().sync_all();
        });

        Ok(Self {
            sender,
            writer,
            path: path.to_path_buf(),
            stop_signal,
            writer_thread: Some(writer_thread),
            flush_thread: Some(flush_thread),
        })
    }

    pub fn log(&self, entry: &mut BDBLogEntry) -> io::Result<()> {
        let mut buf = Vec::with_capacity(256);
        entry.write(&mut buf)?;
        self.sender.send(buf).map_err(|e| io::Error::new(io::ErrorKind::BrokenPipe, e))
    }

    pub fn read_all(&self) -> io::Result<Vec<BDBLogEntry>> {
        {
            let mut w = self.writer.lock().unwrap();
            w.flush()?;
            w.get_ref().sync_all()?;
        }

        let file = File::open(&self.path)?;
        let mut reader = BufReader::new(file);
        let mut entries = Vec::new();

        while let Ok(entry) = BDBLogEntry::read(&mut reader, crate::core::format::BDB_VERSION) {
            entries.push(entry);
        }

        Ok(entries)
    }

    pub fn truncate(&mut self) -> io::Result<()> {
        let mut w = self.writer.lock().unwrap();
        w.flush()?;
        let file = w.get_mut();
        
        retry_on_permission_denied(|| {
            file.set_len(0)?;
            file.sync_all()
        })
    }

    pub fn stop_flush_thread(&mut self) {
        self.stop_signal.store(true, Ordering::Relaxed);
        if let Some(handle) = self.writer_thread.take() {
            let _ = handle.join();
        }
        if let Some(handle) = self.flush_thread.take() {
            let _ = handle.join();
        }
    }
}

#[cfg(windows)]
fn retry_on_permission_denied<F, T>(mut f: F) -> io::Result<T>
where
    F: FnMut() -> io::Result<T>,
{
    let mut attempts = 0;
    loop {
        match f() {
            Ok(res) => return Ok(res),
            Err(e) if e.kind() == io::ErrorKind::PermissionDenied && attempts < 50 => {
                attempts += 1;
                std::thread::sleep(std::time::Duration::from_millis(100));
            }
            Err(e) => {
                if attempts > 0 || e.kind() == io::ErrorKind::PermissionDenied {
                    eprintln!("BrowserDB FATAL Windows Lock Error (WAL) after {} attempts: {}", attempts, e);
                }
                return Err(e);
            }
        }
    }
}

#[cfg(not(windows))]
fn retry_on_permission_denied<F, T>(mut f: F) -> io::Result<T>
where
    F: FnMut() -> io::Result<T>,
{
    f()
}

impl Drop for WALManager {
    fn drop(&mut self) {
        self.stop_signal.store(true, Ordering::Relaxed);
        if let Some(handle) = self.writer_thread.take() {
            let _ = handle.join();
        }
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
