pub mod core;
pub mod sql;

use std::path::Path;
use std::sync::Arc;
use std::fs;
use crate::core::format::{EntryType};
use serde::{Serialize, Deserialize};

pub use crate::core::modes::{DatabaseMode, ModeConfig};
use crate::core::modes::{ModeSwitcher, CurrentMode, PersistentMode};

pub mod types {
    pub use super::{HistoryEntry, CookieEntry, CacheEntry, LocalStoreEntry, SettingEntry};
}

pub mod cookie_flags {
    pub const NONE: u8 = 0;
    pub const SECURE: u8 = 1;
    pub const HTTPONLY: u8 = 2;
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HistoryEntry {
    pub timestamp: u128,
    pub url: String,
    pub url_hash: u128,
    pub title: String,
    pub visit_count: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CookieEntry {
    pub domain_hash: u128,
    pub name: String,
    pub value: String,
    pub expiry: u64,
    pub flags: u8,
}

impl CookieEntry {
    pub fn new(domain_hash: u128, name: String, value: String, expiry: u64) -> Self {
        Self {
            domain_hash,
            name,
            value,
            expiry,
            flags: 0,
        }
    }
    pub fn set_secure(&mut self) { self.flags |= 1; }
    pub fn set_httponly(&mut self) { self.flags |= 2; }
    pub fn is_secure(&self) -> bool { (self.flags & 1) != 0 }
    pub fn is_httponly(&self) -> bool { (self.flags & 2) != 0 }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CacheEntry {
    pub url_hash: u128,
    pub headers: String,
    pub body: Vec<u8>,
    pub etag: String,
    pub last_modified: u128,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LocalStoreEntry {
    pub origin_hash: u128,
    pub key: String,
    pub value: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SettingEntry {
    pub key: String,
    pub value: String,
}

pub struct BrowserDB {
    switcher: Arc<ModeSwitcher>,
}

impl BrowserDB {
    pub fn open<P: AsRef<Path>>(path: P) -> Result<Self, Box<dyn std::error::Error>> {
        Self::open_with_encryption(path, None)
    }

    pub fn open_with_encryption<P: AsRef<Path>>(path: P, encryption_key: Option<[u8; 32]>) -> Result<Self, Box<dyn std::error::Error>> {
        let path = path.as_ref();
        if !path.exists() {
            fs::create_dir_all(path)?;
        }

        let config = ModeConfig {
            max_memory: 1024 * 1024 * 100, // 100MB Default
            enable_compression: false,
            enable_heat_tracking: true,
        };
        
        let switcher = ModeSwitcher::new(path, DatabaseMode::Persistent, config.clone());

        if let Some(key) = encryption_key {
            let mut current = switcher.current_mode.write();
            *current = CurrentMode::Persistent(PersistentMode::new_with_encryption(path, &config, Some(key)));
        }

        Ok(Self {
            switcher: Arc::new(switcher),
        })
    }

    pub fn history(&self) -> HistoryTable<'_> { HistoryTable { db: self } }
    pub fn cookies(&self) -> CookiesTable<'_> { CookiesTable { db: self } }
    pub fn cache(&self) -> CacheTable<'_> { CacheTable { db: self } }
    pub fn localstore(&self) -> LocalStoreTable<'_> { LocalStoreTable { db: self } }
    pub fn settings(&self) -> SettingsTable<'_> { SettingsTable { db: self } }

    pub fn new_batch(&self) -> Batch<'_> { Batch::new(self) }
    
    pub fn sql(self: Arc<Self>) -> sql::SqlEngine {
        sql::SqlEngine::new(self)
    }

    pub fn set_mode(&self, mode: DatabaseMode) -> Result<(), Box<dyn std::error::Error>> {
        let path = self.switcher.base_path.clone();
        self.switcher.switch_mode(mode, &path)?;
        Ok(())
    }

    pub(crate) fn put_raw_localstore(&self, key: Vec<u8>, value: Vec<u8>) -> Result<(), Box<dyn std::error::Error>> {
        match &*self.switcher.current_mode.read() {
            CurrentMode::Persistent(pm) => pm.localstore.put(key, value)?,
            CurrentMode::Ultra(um) => um.localstore.put(key, value),
        }
        Ok(())
    }

    pub(crate) fn get_raw_localstore(&self, key: &[u8]) -> Result<Option<Vec<u8>>, Box<dyn std::error::Error>> {
        let value_opt = match &*self.switcher.current_mode.read() {
            CurrentMode::Persistent(pm) => pm.localstore.get(key).map(|e| e.value),
            CurrentMode::Ultra(um) => um.localstore.get(key),
        };
        Ok(value_opt)
    }

    pub fn stats(&self) -> Result<DatabaseStats, Box<dyn std::error::Error>> {
        let history = self.history().count()? as u64;
        let cookies = self.cookies().count()? as u64;
        let cache = self.cache().count()? as u64;
        let localstore = self.localstore().count()? as u64;
        let settings = self.settings().count()? as u64;

        let mut disk_usage = 0;
        if let Ok(entries) = fs::read_dir(&self.switcher.base_path) {
            for entry in entries.flatten() {
                if let Ok(metadata) = entry.metadata() {
                    disk_usage += metadata.len();
                }
            }
        }

        Ok(DatabaseStats {
            total_entries: history + cookies + cache + localstore + settings,
            history_entries: history,
            cookie_entries: cookies,
            cache_entries: cache,
            localstore_entries: localstore,
            settings_entries: settings,
            memory_usage_mb: 0, // In-memory tables (Ultra) and MemTables are not easily tracked yet
            disk_usage_mb: disk_usage / 1024 / 1024,
        })
    }
    
    pub fn backup<P: AsRef<Path>>(&self, backup_path: P) -> Result<(), Box<dyn std::error::Error>> {
        let backup_path = backup_path.as_ref();
        if !backup_path.exists() {
            fs::create_dir_all(backup_path)?;
        }

        let base_path = &self.switcher.base_path;
        for entry in fs::read_dir(base_path)? {
            let entry = entry?;
            let path = entry.path();
            if path.is_file() {
                let file_name = path.file_name().ok_or("Invalid filename")?;
                let dest_path = backup_path.join(file_name);
                fs::copy(&path, &dest_path)?;
            }
        }
        Ok(())
    }

    pub fn repair(&self) -> Result<usize, Box<dyn std::error::Error>> {
        let mut repaired_count = 0;
        let base_path = &self.switcher.base_path;

        // Simplified repair: check all .sst files
        for entry in fs::read_dir(base_path)? {
            let entry = entry?;
            let path = entry.path();
            if path.extension().map_or(false, |ext| ext == "sst") {
                // If we can't open it, it might be corrupted
                if let Err(_) = crate::core::lsm_tree::SSTable::open(path.clone(), 0) {
                    // In a real system, we might try to recover data from the file
                    // For now, we just move it to a .corrupted file and mark it for repair
                    let mut new_path = path.clone();
                    new_path.set_extension("sst.corrupted");
                    fs::rename(&path, &new_path)?;
                    repaired_count += 1;
                }
            }
        }
        Ok(repaired_count)
    }

    pub fn wipe(&self) -> Result<(), Box<dyn std::error::Error>> {
        let current_mode = self.switcher.current_mode.read();
        match &*current_mode {
            CurrentMode::Persistent(pm) => {
                pm.history.clear()?;
                pm.cookies.clear()?;
                pm.cache.clear()?;
                pm.localstore.clear()?;
                pm.settings.clear()?;
            },
            CurrentMode::Ultra(um) => {
                um.history.data.write().clear();
                um.history.entry_count.store(0, std::sync::atomic::Ordering::SeqCst);
                um.cookies.data.write().clear();
                um.cookies.entry_count.store(0, std::sync::atomic::Ordering::SeqCst);
                um.cache.data.write().clear();
                um.cache.entry_count.store(0, std::sync::atomic::Ordering::SeqCst);
                um.localstore.data.write().clear();
                um.localstore.entry_count.store(0, std::sync::atomic::Ordering::SeqCst);
                um.settings.data.write().clear();
                um.settings.entry_count.store(0, std::sync::atomic::Ordering::SeqCst);
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone)]
pub struct DatabaseStats {
    pub total_entries: u64,
    pub history_entries: u64,
    pub cookie_entries: u64,
    pub cache_entries: u64,
    pub localstore_entries: u64,
    pub settings_entries: u64,
    pub memory_usage_mb: u64,
    pub disk_usage_mb: u64,
}

pub struct HistoryTable<'a> { db: &'a BrowserDB }
impl<'a> HistoryTable<'a> {
    pub fn count(&self) -> Result<usize, Box<dyn std::error::Error>> {
        match &*self.db.switcher.current_mode.read() {
            CurrentMode::Persistent(pm) => Ok(pm.history.all_entries().len()),
            CurrentMode::Ultra(um) => Ok(um.history.entry_count.load(std::sync::atomic::Ordering::SeqCst)),
        }
    }

    pub fn insert(&self, entry: &HistoryEntry) -> Result<(), Box<dyn std::error::Error>> {
        let key = bincode::serialize(&entry.url_hash)?;
        let value = bincode::serialize(entry)?;
        
        match &*self.db.switcher.current_mode.read() {
            CurrentMode::Persistent(pm) => pm.history.put(key, value)?,
            CurrentMode::Ultra(um) => um.history.put(key, value),
        }
        Ok(())
    }
    
    pub fn get(&self, url_hash: u128) -> Result<Option<HistoryEntry>, Box<dyn std::error::Error>> {
        let key = bincode::serialize(&url_hash)?;
        let value_opt = match &*self.db.switcher.current_mode.read() {
            CurrentMode::Persistent(pm) => pm.history.get(&key).map(|e| e.value),
            CurrentMode::Ultra(um) => um.history.get(&key),
        };
        
        if let Some(value) = value_opt {
            let entry = bincode::deserialize(&value)?;
            Ok(Some(entry))
        } else {
            Ok(None)
        }
    }

    pub fn hot_search(&self, _query: &str, _limit: usize) -> Result<Vec<HistoryEntry>, Box<dyn std::error::Error>> {
        Ok(Vec::new())
    }

    pub fn wipe_domain(&self, domain: &str) -> Result<usize, Box<dyn std::error::Error>> {
        let current_mode = self.db.switcher.current_mode.read();
        let all_entries: Vec<(Vec<u8>, Vec<u8>)> = match &*current_mode {
            CurrentMode::Persistent(pm) => {
                pm.history.all_entries().into_iter().map(|e| (e.key, e.value)).collect()
            },
            CurrentMode::Ultra(um) => um.history.all_entries(),
        };

        let mut count = 0;
        for (key, value) in all_entries {
            let entry: HistoryEntry = bincode::deserialize(&value)?;
            if entry.url.contains(domain) {
                match &*current_mode {
                    CurrentMode::Persistent(pm) => pm.history.delete(key)?,
                    CurrentMode::Ultra(um) => um.history.delete(&key),
                }
                count += 1;
            }
        }
        Ok(count)
    }
}

pub struct CookiesTable<'a> { db: &'a BrowserDB }
impl<'a> CookiesTable<'a> {
    pub fn count(&self) -> Result<usize, Box<dyn std::error::Error>> {
        match &*self.db.switcher.current_mode.read() {
            CurrentMode::Persistent(pm) => Ok(pm.cookies.all_entries().len()),
            CurrentMode::Ultra(um) => Ok(um.cookies.entry_count.load(std::sync::atomic::Ordering::SeqCst)),
        }
    }

    pub fn insert(&self, entry: &CookieEntry) -> Result<(), Box<dyn std::error::Error>> {
        let key = bincode::serialize(&(entry.domain_hash, &entry.name))?;
        let value = bincode::serialize(entry)?;
        match &*self.db.switcher.current_mode.read() {
            CurrentMode::Persistent(pm) => pm.cookies.put(key, value)?,
            CurrentMode::Ultra(um) => um.cookies.put(key, value),
        }
        Ok(())
    }
}

pub struct CacheTable<'a> { db: &'a BrowserDB }
impl<'a> CacheTable<'a> {
    pub fn count(&self) -> Result<usize, Box<dyn std::error::Error>> {
        match &*self.db.switcher.current_mode.read() {
            CurrentMode::Persistent(pm) => Ok(pm.cache.all_entries().len()),
            CurrentMode::Ultra(um) => Ok(um.cache.entry_count.load(std::sync::atomic::Ordering::SeqCst)),
        }
    }

    pub fn insert(&self, entry: &CacheEntry) -> Result<(), Box<dyn std::error::Error>> {
        let key = bincode::serialize(&entry.url_hash)?;
        let value = bincode::serialize(entry)?;
        match &*self.db.switcher.current_mode.read() {
            CurrentMode::Persistent(pm) => pm.cache.put(key, value)?,
            CurrentMode::Ultra(um) => um.cache.put(key, value),
        }
        Ok(())
    }

    pub fn get(&self, url_hash: u128) -> Result<Option<CacheEntry>, Box<dyn std::error::Error>> {
        let key = bincode::serialize(&url_hash)?;
        let value_opt = match &*self.db.switcher.current_mode.read() {
            CurrentMode::Persistent(pm) => pm.cache.get(&key).map(|e| e.value),
            CurrentMode::Ultra(um) => um.cache.get(&key),
        };

        if let Some(value) = value_opt {
            let entry = bincode::deserialize(&value)?;
            Ok(Some(entry))
        } else {
            Ok(None)
        }
    }
}

pub struct LocalStoreTable<'a> { db: &'a BrowserDB }
impl<'a> LocalStoreTable<'a> {
    pub fn count(&self) -> Result<usize, Box<dyn std::error::Error>> {
        match &*self.db.switcher.current_mode.read() {
            CurrentMode::Persistent(pm) => Ok(pm.localstore.all_entries().len()),
            CurrentMode::Ultra(um) => Ok(um.localstore.entry_count.load(std::sync::atomic::Ordering::SeqCst)),
        }
    }

    pub fn insert(&self, entry: &LocalStoreEntry) -> Result<(), Box<dyn std::error::Error>> {
        let key = bincode::serialize(&(entry.origin_hash, &entry.key))?;
        let value = bincode::serialize(entry)?;
        match &*self.db.switcher.current_mode.read() {
            CurrentMode::Persistent(pm) => pm.localstore.put(key, value)?,
            CurrentMode::Ultra(um) => um.localstore.put(key, value),
        }
        Ok(())
    }

    pub fn get_by_origin(&self, origin_hash: u128) -> Result<Vec<LocalStoreEntry>, Box<dyn std::error::Error>> {
        let prefix = bincode::serialize(&origin_hash)?;

        let values: Vec<Vec<u8>> = match &*self.db.switcher.current_mode.read() {
            CurrentMode::Persistent(pm) => {
                pm.localstore.scan_prefix(&prefix).into_iter().map(|e| e.value).collect()
            },
            CurrentMode::Ultra(um) => {
                // For Ultra mode (HashMap), we still need to filter all entries unless we change the storage
                um.localstore.all_entries().into_iter()
                    .filter(|(k, _)| k.starts_with(&prefix))
                    .map(|(_, v)| v)
                    .collect()
            }
        };

        let mut results = Vec::new();
        for value in values {
            let entry: LocalStoreEntry = bincode::deserialize(&value)?;
            results.push(entry);
        }
        Ok(results)
    }
}

pub struct Batch<'a> {
    db: &'a BrowserDB,
    operations: Vec<(TableType, EntryType, Vec<u8>, Vec<u8>)>,
}

#[derive(Debug, Clone, Copy)]
pub enum TableType {
    History,
    Cookies,
    Cache,
    LocalStore,
    Settings,
}

impl<'a> Batch<'a> {
    pub fn new(db: &'a BrowserDB) -> Self {
        Self { db, operations: Vec::new() }
    }

    pub fn put_history(&mut self, entry: &HistoryEntry) -> Result<&mut Self, Box<dyn std::error::Error>> {
        let key = bincode::serialize(&entry.url_hash)?;
        let value = bincode::serialize(entry)?;
        self.operations.push((TableType::History, EntryType::Insert, key, value));
        Ok(self)
    }

    pub fn set_setting(&mut self, key: &str, value: &str) -> &mut Self {
        self.operations.push((TableType::Settings, EntryType::Insert, key.as_bytes().to_vec(), value.as_bytes().to_vec()));
        self
    }

    pub fn delete_setting(&mut self, key: &str) -> &mut Self {
        self.operations.push((TableType::Settings, EntryType::Delete, key.as_bytes().to_vec(), Vec::new()));
        self
    }

    pub fn commit(self) -> Result<(), Box<dyn std::error::Error>> {
        // Group by table
        let mut history_ops = Vec::new();
        let mut cookies_ops = Vec::new();
        let mut cache_ops = Vec::new();
        let mut localstore_ops = Vec::new();
        let mut settings_ops = Vec::new();

        for (table, entry_type, key, value) in self.operations {
            match table {
                TableType::History => history_ops.push((entry_type, key, value)),
                TableType::Cookies => cookies_ops.push((entry_type, key, value)),
                TableType::Cache => cache_ops.push((entry_type, key, value)),
                TableType::LocalStore => localstore_ops.push((entry_type, key, value)),
                TableType::Settings => settings_ops.push((entry_type, key, value)),
            }
        }

        let current_mode = self.db.switcher.current_mode.read();
        match &*current_mode {
            CurrentMode::Persistent(pm) => {
                if !history_ops.is_empty() { pm.history.apply_batch(history_ops)?; }
                if !cookies_ops.is_empty() { pm.cookies.apply_batch(cookies_ops)?; }
                if !cache_ops.is_empty() { pm.cache.apply_batch(cache_ops)?; }
                if !localstore_ops.is_empty() { pm.localstore.apply_batch(localstore_ops)?; }
                if !settings_ops.is_empty() { pm.settings.apply_batch(settings_ops)?; }
            },
            CurrentMode::Ultra(um) => {
                for (op, key, value) in history_ops {
                    match op {
                        EntryType::Delete => um.history.delete(&key),
                        _ => um.history.put(key, value),
                    }
                }
                for (op, key, value) in cookies_ops {
                    match op {
                        EntryType::Delete => um.cookies.delete(&key),
                        _ => um.cookies.put(key, value),
                    }
                }
                for (op, key, value) in cache_ops {
                    match op {
                        EntryType::Delete => um.cache.delete(&key),
                        _ => um.cache.put(key, value),
                    }
                }
                for (op, key, value) in localstore_ops {
                    match op {
                        EntryType::Delete => um.localstore.delete(&key),
                        _ => um.localstore.put(key, value),
                    }
                }
                for (op, key, value) in settings_ops {
                    match op {
                        EntryType::Delete => um.settings.delete(&key),
                        _ => um.settings.put(key, value),
                    }
                }
            }
        }
        Ok(())
    }
}

pub struct SettingsTable<'a> { db: &'a BrowserDB }
impl<'a> SettingsTable<'a> {
    pub fn count(&self) -> Result<usize, Box<dyn std::error::Error>> {
        match &*self.db.switcher.current_mode.read() {
            CurrentMode::Persistent(pm) => Ok(pm.settings.all_entries().len()),
            CurrentMode::Ultra(um) => Ok(um.settings.entry_count.load(std::sync::atomic::Ordering::SeqCst)),
        }
    }
    pub fn set(&self, key: &str, value: &str) -> Result<(), Box<dyn std::error::Error>> {
        let k = key.as_bytes().to_vec();
        let v = value.as_bytes().to_vec();
        match &*self.db.switcher.current_mode.read() {
            CurrentMode::Persistent(pm) => pm.settings.put(k, v)?,
            CurrentMode::Ultra(um) => um.settings.put(k, v),
        }
        Ok(())
    }
    
    pub fn get(&self, key: &str) -> Result<Option<String>, Box<dyn std::error::Error>> {
        let k = key.as_bytes();
        let value_opt = match &*self.db.switcher.current_mode.read() {
            CurrentMode::Persistent(pm) => pm.settings.get(k).map(|e| e.value),
            CurrentMode::Ultra(um) => um.settings.get(k),
        };
        
        if let Some(v) = value_opt {
            Ok(Some(String::from_utf8(v)?))
        } else {
            Ok(None)
        }
    }
}
