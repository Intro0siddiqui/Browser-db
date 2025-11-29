pub mod core;
pub mod sql;

use std::path::Path;
use std::sync::Arc;
use std::fs;
use serde::{Serialize, Deserialize};

use crate::core::modes::{ModeSwitcher, ModeConfig, DatabaseMode, CurrentMode};

pub mod types {
    pub use super::{HistoryEntry, CookieEntry, CacheEntry, LocalStoreEntry, SettingEntry};
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HistoryEntry {
    pub timestamp: u128,
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
        let path = path.as_ref();
        if !path.exists() {
            fs::create_dir_all(path)?;
        }

        let config = ModeConfig {
            max_memory: 1024 * 1024 * 100, // 100MB Default
            enable_compression: false,
            enable_heat_tracking: true,
        };
        
        let switcher = ModeSwitcher::new(path, DatabaseMode::Persistent, config);
        Ok(Self {
            switcher: Arc::new(switcher),
        })
    }

    pub fn history(&self) -> HistoryTable<'_> { HistoryTable { db: self } }
    pub fn cookies(&self) -> CookiesTable<'_> { CookiesTable { db: self } }
    pub fn cache(&self) -> CacheTable<'_> { CacheTable { db: self } }
    pub fn localstore(&self) -> LocalStoreTable<'_> { LocalStoreTable { db: self } }
    pub fn settings(&self) -> SettingsTable<'_> { SettingsTable { db: self } }
    
    pub fn sql(self: Arc<Self>) -> sql::SqlEngine {
        sql::SqlEngine::new(self)
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
        Ok(DatabaseStats {
            total_entries: 0,
            history_entries: 0,
            cookie_entries: 0,
            cache_entries: 0,
            memory_usage_mb: 0,
            disk_usage_mb: 0,
        })
    }
    
    pub fn wipe(&self) -> Result<(), Box<dyn std::error::Error>> {
        Ok(())
    }
}

#[derive(Debug, Clone)]
pub struct DatabaseStats {
    pub total_entries: u64,
    pub history_entries: u64,
    pub cookie_entries: u64,
    pub cache_entries: u64,
    pub memory_usage_mb: u64,
    pub disk_usage_mb: u64,
}

pub struct HistoryTable<'a> { db: &'a BrowserDB }
impl<'a> HistoryTable<'a> {
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
}

pub struct CookiesTable<'a> { db: &'a BrowserDB }
impl<'a> CookiesTable<'a> {
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
    pub fn insert(&self, entry: &CacheEntry) -> Result<(), Box<dyn std::error::Error>> {
        let key = bincode::serialize(&entry.url_hash)?;
        let value = bincode::serialize(entry)?;
        match &*self.db.switcher.current_mode.read() {
            CurrentMode::Persistent(pm) => pm.cache.put(key, value)?,
            CurrentMode::Ultra(um) => um.cache.put(key, value),
        }
        Ok(())
    }
}

pub struct LocalStoreTable<'a> { db: &'a BrowserDB }
impl<'a> LocalStoreTable<'a> {
    pub fn insert(&self, entry: &LocalStoreEntry) -> Result<(), Box<dyn std::error::Error>> {
        let key = bincode::serialize(&(entry.origin_hash, &entry.key))?;
        let value = bincode::serialize(entry)?;
        match &*self.db.switcher.current_mode.read() {
            CurrentMode::Persistent(pm) => pm.localstore.put(key, value)?,
            CurrentMode::Ultra(um) => um.localstore.put(key, value),
        }
        Ok(())
    }
}

pub struct SettingsTable<'a> { db: &'a BrowserDB }
impl<'a> SettingsTable<'a> {
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