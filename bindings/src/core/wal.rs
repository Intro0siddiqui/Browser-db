use std::fs::{File, OpenOptions};
use std::io::{self, Write, Cursor, Read};
use std::path::{Path, PathBuf};
use crate::core::format::{BDBLogEntry, EntryType};
use aes_gcm::{
    aead::{Aead, KeyInit},
    Aes256Gcm, Nonce
};

pub struct WAL {
    file: File,
    path: PathBuf,
    encryption_key: Option<[u8; 32]>,
}

impl WAL {
    pub fn new(path: &Path) -> io::Result<Self> {
        Self::new_with_encryption(path, None)
    }

    pub fn new_with_encryption(path: &Path, encryption_key: Option<[u8; 32]>) -> io::Result<Self> {
        let file = OpenOptions::new()
            .append(true)
            .create(true)
            .open(path)?;

        Ok(Self {
            file,
            path: path.to_path_buf(),
            encryption_key,
        })
    }

    pub fn append(&mut self, entry_type: EntryType, key: &[u8], value: &[u8]) -> io::Result<()> {
        let mut entry = BDBLogEntry::new(entry_type, key.to_vec(), value.to_vec());
        let mut buffer = Vec::new();
        entry.write(&mut buffer)?;

        if let Some(enc_key) = &self.encryption_key {
            let cipher = Aes256Gcm::new_from_slice(enc_key).map_err(|_| io::Error::new(io::ErrorKind::Other, "Invalid key"))?;
            let mut nonce_bytes = [0u8; 12];
            rand::RngCore::fill_bytes(&mut rand::thread_rng(), &mut nonce_bytes);
            let nonce = Nonce::from_slice(&nonce_bytes);
            let ciphertext = cipher.encrypt(nonce, buffer.as_slice()).map_err(|_| io::Error::new(io::ErrorKind::Other, "Encryption failed"))?;

            self.file.write_all(&(ciphertext.len() as u32 + 12).to_le_bytes())?;
            self.file.write_all(&nonce_bytes)?;
            self.file.write_all(&ciphertext)?;
        } else {
            self.file.write_all(&(buffer.len() as u32).to_le_bytes())?;
            self.file.write_all(&buffer)?;
        }

        self.file.sync_all()?;
        Ok(())
    }

    pub fn recover(path: &Path, encryption_key: Option<[u8; 32]>) -> io::Result<Vec<BDBLogEntry>> {
        if !path.exists() {
            return Ok(Vec::new());
        }

        let mut file = File::open(path)?;
        let mut entries = Vec::new();

        loop {
            let mut len_bytes = [0u8; 4];
            if file.read_exact(&mut len_bytes).is_err() { break; }
            let len = u32::from_le_bytes(len_bytes) as usize;

            let mut data = vec![0u8; len];
            file.read_exact(&mut data)?;

            let decrypted_data = if let Some(enc_key) = &encryption_key {
                if len < 12 { return Err(io::Error::new(io::ErrorKind::InvalidData, "WAL entry too short")); }
                let cipher = Aes256Gcm::new_from_slice(enc_key).map_err(|_| io::Error::new(io::ErrorKind::Other, "Invalid key"))?;
                let nonce = Nonce::from_slice(&data[..12]);
                cipher.decrypt(nonce, &data[12..]).map_err(|_| io::Error::new(io::ErrorKind::Other, "Decryption failed"))?
            } else {
                data
            };

            let mut cursor = Cursor::new(decrypted_data);
            match BDBLogEntry::read(&mut cursor) {
                Ok(entry) => entries.push(entry),
                Err(e) => return Err(e),
            }
        }

        Ok(entries)
    }

    pub fn clear(&self) -> io::Result<()> {
        let _ = std::fs::remove_file(&self.path);
        Ok(())
    }
}
