use std::fs::{File, OpenOptions};
use std::io::{self, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use crate::core::format::{BDBLogEntry, EntryType};

pub struct BlobPointer {
    pub offset: u64,
    pub size: u32,
}

impl BlobPointer {
    pub fn encode(&self) -> Vec<u8> {
        let mut buf = Vec::with_capacity(12);
        buf.extend_from_slice(&self.offset.to_le_bytes());
        buf.extend_from_slice(&self.size.to_le_bytes());
        buf
    }

    pub fn decode(buf: &[u8]) -> Option<Self> {
        if buf.len() < 12 { return None; }
        let offset = u64::from_le_bytes(buf[0..8].try_into().unwrap());
        let size = u32::from_le_bytes(buf[8..12].try_into().unwrap());
        Some(Self { offset, size })
    }
}

pub struct BlobLog {
    file: Mutex<File>,
    path: PathBuf,
}

impl BlobLog {
    pub fn open(path: &Path) -> io::Result<Self> {
        let mut file = retry_on_permission_denied(|| {
            OpenOptions::new()
                .create(true)
                .write(true)
                .read(true)
                .open(path)
        })?;
        
        file.seek(SeekFrom::End(0))?;
        
        Ok(Self {
            file: Mutex::new(file),
            path: path.to_path_buf(),
        })
    }

    pub fn put(&self, key: &[u8], value: &[u8]) -> io::Result<BlobPointer> {
        let mut file = self.file.lock().unwrap();
        let offset = file.seek(SeekFrom::End(0))?;

        let mut entry = BDBLogEntry::new(EntryType::BlobIndex, key.to_vec(), value.to_vec());
        let size = entry.write(&mut *file)?;

        Ok(BlobPointer {
            offset,
            size: size as u32,
        })
    }

    pub fn get(&self, ptr: &BlobPointer) -> io::Result<Vec<u8>> {
        let mut file = self.file.lock().unwrap();
        file.seek(SeekFrom::Start(ptr.offset))?;
        let entry = BDBLogEntry::read(&mut *file)?;
        Ok(entry.value)
    }

    pub fn get_path(&self) -> PathBuf {
        self.path.clone()
    }

    pub fn swap_file(&self, new_path: &Path) -> io::Result<()> {
        let mut file = self.file.lock().unwrap();
        
        // On Windows, we must close the handle before renaming.
        // We can do this by opening a temporary dummy file or just dropping the handle.
        // Since we are inside a MutexGuard, we can't easily 'drop' it without replacing it.
        
        let dummy_path = if cfg!(windows) { "nul" } else { "/dev/null" };
        if let Ok(dummy) = OpenOptions::new().read(true).open(dummy_path) {
            *file = dummy;
        }

        retry_on_permission_denied(|| std::fs::rename(new_path, &self.path))?;

        let new_file = OpenOptions::new()
            .create(true)
            .append(true)
            .read(true)
            .open(&self.path)?;

        *file = new_file;
        Ok(())
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
                    eprintln!("BrowserDB FATAL Windows Lock Error (BLOB) after {} attempts: {}", attempts, e);
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

pub struct BlobLogIterator {
    file: File,
    offset: u64,
    file_size: u64,
}

impl BlobLogIterator {
    pub fn new(path: &Path) -> io::Result<Self> {
        let mut file = File::open(path)?;
        let file_size = file.seek(SeekFrom::End(0))?;
        file.seek(SeekFrom::Start(0))?;
        Ok(Self {
            file,
            offset: 0,
            file_size,
        })
    }
}

impl Iterator for BlobLogIterator {
    type Item = io::Result<(u64, u32, Vec<u8>, Vec<u8>)>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.offset >= self.file_size {
            return None;
        }

        let current_offset = self.offset;
        match BDBLogEntry::read(&mut self.file) {
            Ok(entry) => {
                match self.file.seek(SeekFrom::Current(0)) {
                    Ok(new_offset) => {
                        let size = (new_offset - current_offset) as u32;
                        self.offset = new_offset;
                        Some(Ok((current_offset, size, entry.key, entry.value)))
                    }
                    Err(e) => Some(Err(e)),
                }
            }
            Err(e) => {
                if self.offset < self.file_size {
                    Some(Err(e))
                } else {
                    None
                }
            }
        }
    }
}
