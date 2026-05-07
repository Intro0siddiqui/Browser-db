pub mod core;
pub mod sql;

use std::path::Path;
use std::sync::Arc;
use std::fs;
use serde::{Serialize, Deserialize};

pub use crate::core::modes::{DatabaseMode, ModeConfig};
use crate::core::modes::{ModeSwitcher, CurrentMode};

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

    pub fn set_mode(&self, mode: DatabaseMode) -> Result<(), Box<dyn std::error::Error>> {
        let path = self.switcher.base_path.clone();
        self.switcher.switch_mode(mode, &path);
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
            memory_usage_mb: 0, // In-memory tables (Ultra) and MemTables are not easily tracked yet
            disk_usage_mb: disk_usage / 1024 / 1024,
        })
    }
    
    pub fn wipe(&self) -> Result<(), Box<dyn std::error::Error>> {
        match &*self.switcher.current_mode.read() {
            CurrentMode::Persistent(pm) => {
                let keys: Vec<Vec<u8>> = pm.history.all_entries().into_iter().map(|e| e.key).collect();
                for k in keys { pm.history.delete(k)?; }
                let keys: Vec<Vec<u8>> = pm.cookies.all_entries().into_iter().map(|e| e.key).collect();
                for k in keys { pm.cookies.delete(k)?; }
                let keys: Vec<Vec<u8>> = pm.cache.all_entries().into_iter().map(|e| e.key).collect();
                for k in keys { pm.cache.delete(k)?; }
                let keys: Vec<Vec<u8>> = pm.localstore.all_entries().into_iter().map(|e| e.key).collect();
                for k in keys { pm.localstore.delete(k)?; }
                let keys: Vec<Vec<u8>> = pm.settings.all_entries().into_iter().map(|e| e.key).collect();
                for k in keys { pm.settings.delete(k)?; }
            },
            CurrentMode::Ultra(um) => {
                um.history.data.write().clear();
                um.cookies.data.write().clear();
                um.cache.data.write().clear();
                um.localstore.data.write().clear();
                um.settings.data.write().clear();
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
    pub memory_usage_mb: u64,
    pub disk_usage_mb: u64,
}

pub struct HistoryTable<'a> { db: &'a BrowserDB }
impl<'a> HistoryTable<'a> {
    pub fn count(&self) -> Result<usize, Box<dyn std::error::Error>> {
        match &*self.db.switcher.current_mode.read() {
            CurrentMode::Persistent(pm) => Ok(pm.history.all_entries().len()),
            CurrentMode::Ultra(um) => Ok(um.history.all_entries().len()),
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
        let all_entries: Vec<(Vec<u8>, Vec<u8>)> = match &*self.db.switcher.current_mode.read() {
            CurrentMode::Persistent(pm) => {
                pm.history.all_entries().into_iter().map(|e| (e.key, e.value)).collect()
            },
            CurrentMode::Ultra(um) => um.history.all_entries(),
        };

        let mut count = 0;
        for (key, value) in all_entries {
            let entry: HistoryEntry = bincode::deserialize(&value)?;
            if entry.title.contains(domain) {
                match &*self.db.switcher.current_mode.read() {
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
            CurrentMode::Ultra(um) => Ok(um.cookies.all_entries().len()),
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
            CurrentMode::Ultra(um) => Ok(um.cache.all_entries().len()),
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
            CurrentMode::Ultra(um) => Ok(um.localstore.all_entries().len()),
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
        let all_entries: Vec<Vec<u8>> = match &*self.db.switcher.current_mode.read() {
            CurrentMode::Persistent(pm) => {
                pm.localstore.all_entries().into_iter().map(|e| e.value).collect()
            },
            CurrentMode::Ultra(um) => {
                um.localstore.all_entries().into_iter().map(|(_, v)| v).collect()
            }
        };

        let mut results = Vec::new();
        for value in all_entries {
            let entry: LocalStoreEntry = bincode::deserialize(&value)?;
            if entry.origin_hash == origin_hash {
                results.push(entry);
            }
        }
        Ok(results)
    }
}

pub struct SettingsTable<'a> { db: &'a BrowserDB }
impl<'a> SettingsTable<'a> {
    pub fn count(&self) -> Result<usize, Box<dyn std::error::Error>> {
        match &*self.db.switcher.current_mode.read() {
            CurrentMode::Persistent(pm) => Ok(pm.settings.all_entries().len()),
            CurrentMode::Ultra(um) => Ok(um.settings.all_entries().len()),
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
