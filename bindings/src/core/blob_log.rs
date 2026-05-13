use std::fs::{File, OpenOptions};
use std::io::{self, Read, Write, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

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
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .read(true)
            .open(path)?;
        Ok(Self {
            file: Mutex::new(file),
            path: path.to_path_buf(),
        })
    }

    pub fn put(&self, value: &[u8]) -> io::Result<BlobPointer> {
        let mut file = self.file.lock().unwrap();
        let offset = file.seek(SeekFrom::End(0))?;
        file.write_all(value)?;
        Ok(BlobPointer {
            offset,
            size: value.len() as u32,
        })
    }

    pub fn get(&self, ptr: &BlobPointer) -> io::Result<Vec<u8>> {
        let mut file = self.file.lock().unwrap();
        let mut buf = vec![0u8; ptr.size as usize];
        file.seek(SeekFrom::Start(ptr.offset))?;
        file.read_exact(&mut buf)?;
        Ok(buf)
    }
}
