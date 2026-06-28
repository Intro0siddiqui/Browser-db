pub mod core;
pub mod ffi;

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::collections::HashMap;
use std::{fs::{self, File}, io};
use parking_lot::RwLock;
use serde::{Serialize, Deserialize};
use fs2::FileExt;

pub use crate::core::modes::{DatabaseMode, ModeConfig};
use crate::core::modes::{ModeSwitcher, CurrentMode};
use crate::core::config::BrowserDBConfig;

pub mod types {
    pub use super::{
        HistoryEntry, CookieEntry, CacheEntry, LocalStoreEntry, SettingEntry, BookmarkEntry,
        HistoryEntryRef, CookieEntryRef, CacheEntryRef, LocalStoreEntryRef, SettingEntryRef, BookmarkEntryRef,
    };
}

pub mod cookie_flags {
    pub const NONE: u8 = 0;
    pub const SECURE: u8 = 1;
    pub const HTTPONLY: u8 = 2;
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BookmarkEntry {
    pub url_hash: u128,
    pub url: String,
    pub title: String,
    pub folder: String,
    pub created_at: u64,
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
pub struct HistoryEntryRef<'a> {
    pub timestamp: u128,
    pub url: &'a str,
    pub url_hash: u128,
    pub title: &'a str,
    pub visit_count: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CookieEntry {
    pub domain_hash: u128,
    pub name: String,
    pub value: String,
    pub path: String,
    pub domain: String,
    pub expiry: u64,
    pub flags: u8,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CookieEntryRef<'a> {
    pub domain_hash: u128,
    pub name: &'a str,
    pub value: &'a str,
    pub path: &'a str,
    pub domain: &'a str,
    pub expiry: u64,
    pub flags: u8,
}

impl CookieEntry {
    pub fn new(domain_hash: u128, name: String, value: String, expiry: u64) -> Self {
        Self {
            domain_hash,
            name,
            value,
            path: String::new(),
            domain: String::new(),
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
pub struct CacheEntryRef<'a> {
    pub url_hash: u128,
    pub headers: &'a str,
    pub body: &'a [u8],
    pub etag: &'a str,
    pub last_modified: u128,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LocalStoreEntry {
    pub origin_hash: u128,
    pub key: String,
    pub value: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LocalStoreEntryRef<'a> {
    pub origin_hash: u128,
    pub key: &'a str,
    pub value: &'a str,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SettingEntry {
    pub key: String,
    pub value: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SettingEntryRef<'a> {
    pub key: &'a str,
    pub value: &'a str,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BookmarkEntryRef<'a> {
    pub url_hash: u128,
    pub url: &'a str,
    pub title: &'a str,
    pub folder: &'a str,
    pub created_at: u64,
}

pub struct Container {
    pub name: String,
    pub switcher: Arc<ModeSwitcher>,
    pub pku: u16, // Hardware Protection Key for Hajr HAL
}

impl Container {
    pub fn history(&self) -> HistoryTable<'_> { HistoryTable { container: self } }
    pub fn bookmarks(&self) -> BookmarksTable<'_> { BookmarksTable { container: self } }
    pub fn cookies(&self) -> CookiesTable<'_> { CookiesTable { container: self } }
    pub fn cache(&self) -> CacheTable<'_> { CacheTable { container: self } }
    pub fn localstore(&self) -> LocalStoreTable<'_> { LocalStoreTable { container: self } }
    pub fn settings(&self) -> SettingsTable<'_> { SettingsTable { container: self } }
    pub fn binarystore(&self) -> BinaryStoreTable<'_> { BinaryStoreTable { container: self } }

    pub fn set_mode(&self, mode: DatabaseMode) -> Result<(), Box<dyn std::error::Error>> {
        let path = self.switcher.base_path.clone();
        self.switcher.switch_mode(mode, &path)?;
        Ok(())
    }

    pub fn wipe(&self) -> Result<(), Box<dyn std::error::Error>> {
        let current_mode = self.switcher.current_mode.read();
        match &*current_mode {
            CurrentMode::Persistent(pm) => {
                pm.history.clear()?;
                pm.bookmarks.clear()?;
                pm.cookies.clear()?;
                pm.cache.clear()?;
                pm.localstore.clear()?;
                pm.settings.clear()?;
                pm.binarystore.clear()?;
            },
            CurrentMode::Ultra(um) => {
                um.clear();
            }
        }
        Ok(())
    }

    pub fn stats(&self) -> Result<DatabaseStats, Box<dyn std::error::Error>> {
        let history = self.history().count()? as u64;
        let bookmarks = self.bookmarks().count()? as u64;
        let cookies = self.cookies().count()? as u64;
        let cache = self.cache().count()? as u64;
        let localstore = self.localstore().count()? as u64;
        let settings = self.settings().count()? as u64;
        let binarystore = self.binarystore().count()? as u64;

        let mut disk_usage = 0;
        if let Ok(entries) = fs::read_dir(&self.switcher.base_path) {
            for entry in entries.flatten() {
                if let Ok(metadata) = entry.metadata() {
                    disk_usage += metadata.len();
                }
            }
        }

        Ok(DatabaseStats {
            total_entries: history + bookmarks + cookies + cache + localstore + settings + binarystore,
            history_entries: history,
            bookmark_entries: bookmarks,
            cookie_entries: cookies,
            cache_entries: cache,
            localstore_entries: localstore,
            settings_entries: settings,
            binarystore_entries: binarystore,
            memory_usage_mb: 0,
            disk_usage_mb: disk_usage / 1024 / 1024,
        })
    }
}

pub struct BrowserDB {
    base_path: PathBuf,
    config: ModeConfig,
    containers: RwLock<HashMap<String, Arc<Container>>>,
    default_container: Arc<Container>,
    _lock_file: File,
}

impl BrowserDB {
    pub fn open<P: AsRef<Path>>(path: P) -> Result<Self, Box<dyn std::error::Error>> {
        Self::open_with_locking(path, true)
    }

    pub fn open_without_locking<P: AsRef<Path>>(path: P) -> Result<Self, Box<dyn std::error::Error>> {
        Self::open_with_locking(path, false)
    }

    fn open_with_locking<P: AsRef<Path>>(path: P, use_locking: bool) -> Result<Self, Box<dyn std::error::Error>> {
        let path = path.as_ref();
        if !path.exists() {
            fs::create_dir_all(path)?;
        }

        let lock_path = path.join("browserdb.lock");
        let lock_file = fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .open(lock_path)?;

        if use_locking {
            lock_file.try_lock_exclusive().map_err(|_| {
                io::Error::new(io::ErrorKind::Other, "Database is already in use by another process")
            })?;
        }

        let ext_config = BrowserDBConfig::load_or_default(path);

        let config = ModeConfig {
            max_memory: 1024 * 1024 * 100, // 100MB Default
            enable_compression: false,
            enable_heat_tracking: true,
            ext_config,
        };

        let db = Self {
            base_path: path.to_path_buf(),
            config,
            containers: RwLock::new(HashMap::new()),
            // Temporarily dummy, will be replaced
            default_container: Arc::new(Container {
                name: "dummy".to_string(),
                switcher: Arc::new(ModeSwitcher::new(path, DatabaseMode::Persistent, ModeConfig {
                    max_memory: 0,
                    enable_compression: false,
                    enable_heat_tracking: false,
                    ext_config: BrowserDBConfig::default(),
                })?),
                pku: 0,
            }),
            _lock_file: lock_file,
        };
        
        let default = db.container("default")?;
        Ok(Self {
            default_container: default,
            ..db
        })
    }

    pub fn container(&self, name: &str) -> Result<Arc<Container>, Box<dyn std::error::Error>> {
        // Sanitize name to prevent path traversal
        let sanitized_name: String = name.chars()
            .filter(|c| c.is_alphanumeric() || *c == '_' || *c == '-')
            .collect();

        if sanitized_name.is_empty() || sanitized_name != name {
            return Err("Invalid container name: only alphanumeric, underscore, and hyphen allowed".into());
        }

        {
            let containers = self.containers.read();
            if let Some(c) = containers.get(&sanitized_name) {
                return Ok(Arc::clone(c));
            }
        }

        let mut containers = self.containers.write();
        let container_path = self.base_path.join(format!("container_{}", sanitized_name));
        if !container_path.exists() {
            fs::create_dir_all(&container_path)?;
        }

        let mut index_defs = HashMap::new();
        let ls_indices = vec![
            crate::core::lsm_tree::IndexDefinition {
                name: "value".to_string(),
                field_name: "value".to_string(),
                extractor: Arc::new(LocalStoreTable::extract_value_index),
            },
            crate::core::lsm_tree::IndexDefinition {
                name: "key".to_string(),
                field_name: "key".to_string(),
                extractor: Arc::new(LocalStoreTable::extract_key_index),
            },
            crate::core::lsm_tree::IndexDefinition {
                name: "origin_hash".to_string(),
                field_name: "origin_hash".to_string(),
                extractor: Arc::new(LocalStoreTable::extract_origin_index),
            },
        ];
        index_defs.insert(crate::core::format::TableType::LocalStore, ls_indices);

        let switcher = ModeSwitcher::new_with_indices(&container_path, DatabaseMode::Persistent, self.config.clone(), index_defs)?;

        // Assign a pseudo-PKU based on name hash for Hajr HAL isolation
        let pku = (ffi::calculate_hash(&sanitized_name) % 16) as u16;

        let container = Arc::new(Container {
            name: sanitized_name.clone(),
            switcher: Arc::new(switcher),
            pku,
        });
        containers.insert(sanitized_name, Arc::clone(&container));
        Ok(container)
    }

    pub fn history(&self) -> HistoryTable<'_> {
        HistoryTable { container: &self.default_container }
    }
    pub fn bookmarks(&self) -> BookmarksTable<'_> {
        BookmarksTable { container: &self.default_container }
    }
    pub fn cookies(&self) -> CookiesTable<'_> {
        CookiesTable { container: &self.default_container }
    }
    pub fn cache(&self) -> CacheTable<'_> {
        CacheTable { container: &self.default_container }
    }
    pub fn localstore(&self) -> LocalStoreTable<'_> {
        LocalStoreTable { container: &self.default_container }
    }
    pub fn settings(&self) -> SettingsTable<'_> {
        SettingsTable { container: &self.default_container }
    }
    pub fn binarystore(&self) -> BinaryStoreTable<'_> {
        BinaryStoreTable { container: &self.default_container }
    }

    pub fn set_mode(&self, mode: DatabaseMode) -> Result<(), Box<dyn std::error::Error>> {
        self.default_container.set_mode(mode)
    }

    pub fn stats(&self) -> Result<DatabaseStats, Box<dyn std::error::Error>> {
        self.default_container.stats()
    }
    
    pub fn wipe(&self) -> Result<(), Box<dyn std::error::Error>> {
        self.default_container.wipe()
    }
}

#[derive(Debug, Clone)]
pub struct DatabaseStats {
    pub total_entries: u64,
    pub history_entries: u64,
    pub bookmark_entries: u64,
    pub cookie_entries: u64,
    pub cache_entries: u64,
    pub localstore_entries: u64,
    pub settings_entries: u64,
    pub binarystore_entries: u64,
    pub memory_usage_mb: u64,
    pub disk_usage_mb: u64,
}

pub struct HistoryTable<'a> { container: &'a Container }
impl<'a> HistoryTable<'a> {
    pub fn count(&self) -> Result<usize, Box<dyn std::error::Error>> {
        match &*self.container.switcher.current_mode.read() {
            CurrentMode::Persistent(pm) => Ok(pm.history.all_entries().len()),
            CurrentMode::Ultra(um) => Ok(um.history.all_entries().len()),
        }
    }

    pub fn insert(&self, entry: &HistoryEntry) -> Result<(), Box<dyn std::error::Error>> {
        let key = bincode::serialize(&entry.url_hash)?;
        let value = bincode::serialize(entry)?;
        
        match &*self.container.switcher.current_mode.read() {
            CurrentMode::Persistent(pm) => pm.history.put(key, value)?,
            CurrentMode::Ultra(um) => um.history.put(key, value, 0),
        }
        Ok(())
    }

    /// Inserts a history entry with a Time-To-Live.
    ///
    /// In `CurrentMode::Persistent`, the entry's expiry is stored and
    /// enforced during reads, scans, and compaction.
    /// In `CurrentMode::Ultra`, expiry is enforced lazily on read; a
    /// purge pass is triggered after the write to reclaim memory.
    pub fn insert_with_ttl(&self, entry: &HistoryEntry, ttl_ms: u64) -> Result<(), Box<dyn std::error::Error>> {
        let key = bincode::serialize(&entry.url_hash)?;
        let value = bincode::serialize(entry)?;

        match &*self.container.switcher.current_mode.read() {
            CurrentMode::Persistent(pm) => pm.history.put_with_ttl(key, value, ttl_ms)?,
            CurrentMode::Ultra(um) => {
                let expires_at = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_millis() as u64 + ttl_ms;
                um.history.put(key, value, expires_at);
                um.purge_expired_all();
            }
        }
        Ok(())
    }

    pub fn increment(&self, url_hash: u128, delta: i64) -> Result<(), Box<dyn std::error::Error>> {
        let key = bincode::serialize(&url_hash)?;
        match &*self.container.switcher.current_mode.read() {
            CurrentMode::Persistent(pm) => pm.history.increment(key, delta)?,
            CurrentMode::Ultra(um) => um.history.increment(&key, delta),
        }
        Ok(())
    }
    
    pub fn get(&self, url_hash: u128) -> Result<Option<HistoryEntry>, Box<dyn std::error::Error>> {
        let key = bincode::serialize(&url_hash)?;
        let value_opt = match &*self.container.switcher.current_mode.read() {
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

    /// Search the history table for entries whose `url` or `title` contain
    /// `query` (case-insensitive substring), ranked by "hotness":
    ///
    ///   1. Higher `visit_count` first.
    ///   2. Tiebreak: more recent `timestamp` first.
    ///
    /// Returns at most `limit` entries. An empty `query` matches every
    /// entry and returns them all ranked.
    pub fn hot_search(&self, query: &str, limit: usize) -> Result<Vec<HistoryEntry>, Box<dyn std::error::Error>> {
        let needle = query.to_lowercase();

        let entries: Vec<(Vec<u8>, Vec<u8>)> = match &*self.container.switcher.current_mode.read() {
            CurrentMode::Persistent(pm) => pm.history
                .all_entries()
                .into_iter()
                .map(|e| (e.key, e.value))
                .collect(),
            CurrentMode::Ultra(um) => um.history.all_entries(),
        };

        let mut matched: Vec<HistoryEntry> = Vec::new();
        for (_key, value) in entries {
            if let Ok(entry) = bincode::deserialize::<HistoryEntry>(&value) {
                if needle.is_empty()
                    || entry.url.to_lowercase().contains(&needle)
                    || entry.title.to_lowercase().contains(&needle)
                {
                    matched.push(entry);
                }
            }
        }

        matched.sort_by(|a, b| {
            b.visit_count
                .cmp(&a.visit_count)
                .then(b.timestamp.cmp(&a.timestamp))
        });

        matched.truncate(limit);
        Ok(matched)
    }

    pub fn wipe_domain(&self, domain: &str) -> Result<usize, Box<dyn std::error::Error>> {
        let current_mode = self.container.switcher.current_mode.read();
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

pub struct BookmarksTable<'a> { container: &'a Container }
impl<'a> BookmarksTable<'a> {
    pub fn count(&self) -> Result<usize, Box<dyn std::error::Error>> {
        match &*self.container.switcher.current_mode.read() {
            CurrentMode::Persistent(pm) => Ok(pm.bookmarks.all_entries().len()),
            CurrentMode::Ultra(um) => Ok(um.bookmarks.entry_count.load(std::sync::atomic::Ordering::SeqCst)),
        }
    }

    pub fn insert(&self, entry: &BookmarkEntry) -> Result<(), Box<dyn std::error::Error>> {
        let key = bincode::serialize(&entry.url_hash)?;
        let value = bincode::serialize(entry)?;
        match &*self.container.switcher.current_mode.read() {
            CurrentMode::Persistent(pm) => pm.bookmarks.put(key, value)?,
            CurrentMode::Ultra(um) => um.bookmarks.put(key, value, 0),
        }
        Ok(())
    }

    pub fn delete(&self, url_hash: u128) -> Result<(), Box<dyn std::error::Error>> {
        let key = bincode::serialize(&url_hash)?;
        match &*self.container.switcher.current_mode.read() {
            CurrentMode::Persistent(pm) => pm.bookmarks.delete(key)?,
            CurrentMode::Ultra(um) => um.bookmarks.delete(&key),
        }
        Ok(())
    }

    pub fn get_all(&self) -> Result<Vec<BookmarkEntry>, Box<dyn std::error::Error>> {
        let current_mode = self.container.switcher.current_mode.read();
        let all_entries: Vec<(Vec<u8>, Vec<u8>)> = match &*current_mode {
            CurrentMode::Persistent(pm) => {
                pm.bookmarks.all_entries().into_iter().map(|e| (e.key, e.value)).collect()
            }
            CurrentMode::Ultra(um) => um.bookmarks.all_entries(),
        };

        let mut bookmarks = Vec::with_capacity(all_entries.len());
        for (_key, value) in all_entries {
            if let Ok(entry) = bincode::deserialize::<BookmarkEntry>(&value) {
                bookmarks.push(entry);
            }
        }
        Ok(bookmarks)
    }
}

pub struct CookiesTable<'a> { container: &'a Container }
impl<'a> CookiesTable<'a> {
    pub fn count(&self) -> Result<usize, Box<dyn std::error::Error>> {
        match &*self.container.switcher.current_mode.read() {
            CurrentMode::Persistent(pm) => Ok(pm.cookies.all_entries().len()),
            CurrentMode::Ultra(um) => Ok(um.cookies.entry_count.load(std::sync::atomic::Ordering::SeqCst)),
        }
    }

    pub fn insert(&self, entry: &CookieEntry) -> Result<(), Box<dyn std::error::Error>> {
        let key = bincode::serialize(&(entry.domain_hash, &entry.name))?;
        let value = bincode::serialize(entry)?;
        match &*self.container.switcher.current_mode.read() {
            CurrentMode::Persistent(pm) => pm.cookies.put(key, value)?,
            CurrentMode::Ultra(um) => um.cookies.put(key, value, 0),
        }
        Ok(())
    }

    pub fn delete(&self, domain_hash: u128, name: &str) -> Result<(), Box<dyn std::error::Error>> {
        let key = bincode::serialize(&(domain_hash, name))?;
        match &*self.container.switcher.current_mode.read() {
            CurrentMode::Persistent(pm) => pm.cookies.delete(key)?,
            CurrentMode::Ultra(um) => um.cookies.delete(&key),
        }
        Ok(())
    }

    pub fn get(&self, domain_hash: u128, name: &str) -> Result<Option<CookieEntry>, Box<dyn std::error::Error>> {
        let key = bincode::serialize(&(domain_hash, name))?;
        let value_opt = match &*self.container.switcher.current_mode.read() {
            CurrentMode::Persistent(pm) => pm.cookies.get(&key).map(|e| e.value),
            CurrentMode::Ultra(um) => um.cookies.get(&key),
        };
        if let Some(value) = value_opt {
            let entry = bincode::deserialize(&value)?;
            Ok(Some(entry))
        } else {
            Ok(None)
        }
    }

    pub fn get_by_domain(&self, domain_hash: u128) -> Result<Vec<CookieEntry>, Box<dyn std::error::Error>> {
        let prefix = bincode::serialize(&domain_hash)?;
        let current_mode = self.container.switcher.current_mode.read();
        let values: Vec<Vec<u8>> = match &*current_mode {
            CurrentMode::Persistent(pm) => {
                pm.cookies.scan_prefix(&prefix).into_iter().map(|e| e.value).collect()
            },
            CurrentMode::Ultra(um) => {
                um.cookies.all_entries().into_iter()
                    .filter(|(k, _)| k.starts_with(&prefix))
                    .map(|(_, v)| v)
                    .collect()
            }
        };
        let mut cookies = Vec::with_capacity(values.len());
        for value in values {
            if let Ok(entry) = bincode::deserialize::<CookieEntry>(&value) {
                cookies.push(entry);
            }
        }
        Ok(cookies)
    }

    pub fn get_all(&self) -> Result<Vec<CookieEntry>, Box<dyn std::error::Error>> {
        let current_mode = self.container.switcher.current_mode.read();
        let all_entries: Vec<(Vec<u8>, Vec<u8>)> = match &*current_mode {
            CurrentMode::Persistent(pm) => {
                pm.cookies.all_entries().into_iter().map(|e| (e.key, e.value)).collect()
            }
            CurrentMode::Ultra(um) => um.cookies.all_entries(),
        };

        let mut cookies = Vec::with_capacity(all_entries.len());
        for (_key, value) in all_entries {
            if let Ok(entry) = bincode::deserialize::<CookieEntry>(&value) {
                cookies.push(entry);
            }
        }
        Ok(cookies)
    }
}

pub struct CacheTable<'a> { container: &'a Container }
impl<'a> CacheTable<'a> {
    pub fn count(&self) -> Result<usize, Box<dyn std::error::Error>> {
        match &*self.container.switcher.current_mode.read() {
            CurrentMode::Persistent(pm) => Ok(pm.cache.all_entries().len()),
            CurrentMode::Ultra(um) => Ok(um.cache.entry_count.load(std::sync::atomic::Ordering::SeqCst)),
        }
    }

    pub fn insert(&self, entry: &CacheEntry) -> Result<(), Box<dyn std::error::Error>> {
        let key = bincode::serialize(&entry.url_hash)?;
        let value = bincode::serialize(entry)?;
        match &*self.container.switcher.current_mode.read() {
            CurrentMode::Persistent(pm) => pm.cache.put(key, value)?,
            CurrentMode::Ultra(um) => um.cache.put(key, value, 0),
        }
        Ok(())
    }

    pub fn get(&self, url_hash: u128) -> Result<Option<CacheEntry>, Box<dyn std::error::Error>> {
        let key = bincode::serialize(&url_hash)?;
        let value_opt = match &*self.container.switcher.current_mode.read() {
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

pub struct LocalStoreTable<'a> { container: &'a Container }
impl<'a> LocalStoreTable<'a> {
    pub fn count(&self) -> Result<usize, Box<dyn std::error::Error>> {
        match &*self.container.switcher.current_mode.read() {
            CurrentMode::Persistent(pm) => Ok(pm.localstore.all_entries().len()),
            CurrentMode::Ultra(um) => Ok(um.localstore.entry_count.load(std::sync::atomic::Ordering::SeqCst)),
        }
    }

    pub fn insert(&self, entry: &LocalStoreEntry) -> Result<(), Box<dyn std::error::Error>> {
        let primary_key = bincode::serialize(&(entry.origin_hash, &entry.key))?;
        let value = bincode::serialize(entry)?;

        match &*self.container.switcher.current_mode.read() {
            CurrentMode::Persistent(pm) => pm.localstore.put(primary_key, value)?,
            CurrentMode::Ultra(um) => {
                um.localstore.put(primary_key.clone(), value.clone(), 0);
                // Ultra mode still needs manual indexing for now
                if let Some(idx_key) = Self::extract_value_index(&primary_key, &value) {
                    um.localstore.put(idx_key, primary_key, 0);
                }
            }
        }
        Ok(())
    }

    pub fn remove(&self, origin_hash: u128, key: &str) -> Result<(), Box<dyn std::error::Error>> {
        let primary_key = bincode::serialize(&(origin_hash, key))?;
        match &*self.container.switcher.current_mode.read() {
            CurrentMode::Persistent(pm) => pm.localstore.delete(primary_key)?,
            CurrentMode::Ultra(um) => um.localstore.delete(&primary_key),
        }
        Ok(())
    }

    pub fn clear_origin(&self, origin_hash: u128) -> Result<(), Box<dyn std::error::Error>> {
        let prefix = bincode::serialize(&origin_hash)?;
        match &*self.container.switcher.current_mode.read() {
            CurrentMode::Persistent(pm) => {
                let entries = pm.localstore.scan_prefix(&prefix);
                for entry in entries {
                    pm.localstore.delete(entry.key)?;
                }
            },
            CurrentMode::Ultra(um) => {
                let mut current = um.localstore.data.write();
                current.retain(|k, _| !k.starts_with(&prefix));
            }
        }
        Ok(())
    }

    pub(crate) fn extract_value_index(_k: &[u8], v: &[u8]) -> Option<Vec<u8>> {
        if let Ok(entry) = bincode::deserialize::<LocalStoreEntry>(v) {
            Some(format!("idx:localstore:value:{}:{}", entry.value, entry.key).into_bytes())
        } else {
            None
        }
    }

    pub(crate) fn extract_key_index(_k: &[u8], v: &[u8]) -> Option<Vec<u8>> {
        if let Ok(entry) = bincode::deserialize::<LocalStoreEntry>(v) {
            Some(format!("idx:localstore:key:{}:{}", entry.key, entry.value).into_bytes())
        } else {
            None
        }
    }

    pub(crate) fn extract_origin_index(_k: &[u8], v: &[u8]) -> Option<Vec<u8>> {
        if let Ok(entry) = bincode::deserialize::<LocalStoreEntry>(v) {
            Some(format!("idx:localstore:origin:{}:{}:{}", entry.origin_hash, entry.key, entry.value).into_bytes())
        } else {
            None
        }
    }

    /// Insert a `LocalStoreEntry` and only build the secondary indices whose
    /// logical field name appears in `index_fields`. Supported fields:
    /// `"value"`, `"key"`, `"origin_hash"`. An empty slice keeps the default
    /// behavior (build the `"value"` index). An unknown field name yields
    /// `Err`.
    pub fn insert_with_index(
        &self,
        entry: &LocalStoreEntry,
        index_fields: &[&str],
    ) -> Result<(), Box<dyn std::error::Error>> {
        const SUPPORTED: &[&str] = &["value", "key", "origin_hash"];
        for f in index_fields {
            if !SUPPORTED.contains(f) {
                return Err(format!(
                    "Unknown index field '{}': supported fields are {:?}",
                    f, SUPPORTED
                )
                .into());
            }
        }

        let primary_key = bincode::serialize(&(entry.origin_hash, &entry.key))?;
        let value = bincode::serialize(entry)?;

        let allowed: Option<Vec<&str>> = if index_fields.is_empty() {
            None
        } else {
            Some(index_fields.to_vec())
        };

        match &*self.container.switcher.current_mode.read() {
            CurrentMode::Persistent(pm) => {
                pm.localstore.put_with_field_filter(
                    primary_key,
                    value,
                    allowed.as_deref(),
                )?;
            }
            CurrentMode::Ultra(um) => {
                um.localstore.put(primary_key.clone(), value.clone(), 0);
                for field in allowed.as_deref().unwrap_or(&["value"]) {
                    let idx_key = match *field {
                        "value" => Self::extract_value_index(&primary_key, &value),
                        "key" => Self::extract_key_index(&primary_key, &value),
                        "origin_hash" => Self::extract_origin_index(&primary_key, &value),
                        _ => None,
                    };
                    if let Some(idx_key) = idx_key {
                        um.localstore.put(idx_key, primary_key.clone(), 0);
                    }
                }
            }
        }
        Ok(())
    }

    pub fn get(&self, origin_hash: u128, key: &str) -> Result<Option<LocalStoreEntry>, Box<dyn std::error::Error>> {
        let primary_key = bincode::serialize(&(origin_hash, key))?;
        let value_opt = match &*self.container.switcher.current_mode.read() {
            CurrentMode::Persistent(pm) => pm.localstore.get(&primary_key).map(|e| e.value),
            CurrentMode::Ultra(um) => um.localstore.get(&primary_key),
        };
        if let Some(value) = value_opt {
            let entry = bincode::deserialize::<LocalStoreEntry>(&value)?;
            Ok(Some(entry))
        } else {
            Ok(None)
        }
    }

    pub fn get_by_origin(&self, origin_hash: u128) -> Result<Vec<LocalStoreEntry>, Box<dyn std::error::Error>> {
        let prefix = bincode::serialize(&origin_hash)?;

        let values: Vec<Vec<u8>> = match &*self.container.switcher.current_mode.read() {
            CurrentMode::Persistent(pm) => {
                pm.localstore.scan_prefix(&prefix).into_iter().map(|e| e.value).collect()
            },
            CurrentMode::Ultra(um) => {
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

    pub fn query(&self) -> QueryBuilder<'_, 'a> {
        QueryBuilder::new(self)
    }
}

pub type Filter<'a> = Box<dyn Fn(&LocalStoreEntry) -> bool + 'a>;

pub struct QueryBuilder<'q, 'a> {
    pub table: &'q LocalStoreTable<'a>,
    prefix: Vec<u8>,
    filters: Vec<Filter<'a>>,
    limit: Option<usize>,
    value_eq: Option<String>,
}

impl<'q, 'a> QueryBuilder<'q, 'a> {
    pub fn new(table: &'q LocalStoreTable<'a>) -> Self {
        Self {
            table,
            prefix: Vec::new(),
            filters: Vec::new(),
            limit: None,
            value_eq: None,
        }
    }

    pub fn prefix(mut self, prefix: Vec<u8>) -> Self {
        self.prefix = prefix;
        self
    }

    pub fn filter<F>(mut self, filter: F) -> Self
    where F: Fn(&LocalStoreEntry) -> bool + 'a {
        self.filters.push(Box::new(filter));
        self
    }

    pub fn value_eq(mut self, value: String) -> Self {
        self.value_eq = Some(value);
        self
    }

    pub fn limit(mut self, limit: usize) -> Self {
        self.limit = Some(limit);
        self
    }

    pub fn execute(self) -> Result<Vec<LocalStoreEntry>, Box<dyn std::error::Error>> {
        let current_mode = self.table.container.switcher.current_mode.read();

        let mut results = Vec::new();

        match &*current_mode {
            CurrentMode::Persistent(pm) => {
                // Optimized path if we have value_eq and indices
                if let Some(val) = &self.value_eq {
                    let idx_prefix = format!("idx:localstore:value:{}:", val).into_bytes();

                    // Use the native secondary index
                    let index_tree = pm.localstore.inner.indices.iter().find(|i| i.name == "value");
                    let idx_entries = if let Some(idx) = index_tree {
                        idx.tree.scan_prefix(&idx_prefix)
                    } else {
                        pm.localstore.scan_prefix(&idx_prefix)
                    };

                    for idx_kv in idx_entries {
                        if let Some(primary_kv) = pm.localstore.get(&idx_kv.value) {
                            if let Ok(entry) = bincode::deserialize::<LocalStoreEntry>(&primary_kv.value) {
                                if self.filters.iter().all(|f| f(&entry)) {
                                    results.push(entry);
                                }
                            }
                        }
                        if let Some(l) = self.limit {
                            if results.len() >= l { break; }
                        }
                    }
                } else {
                    // Standard predicate-based scan
                    let kvs = pm.localstore.scan_with_predicate(&self.prefix, |kv| {
                        if let Ok(entry) = bincode::deserialize::<LocalStoreEntry>(&kv.value) {
                            self.filters.iter().all(|f| f(&entry))
                        } else {
                            false
                        }
                    });
                    for kv in kvs {
                        if let Ok(entry) = bincode::deserialize::<LocalStoreEntry>(&kv.value) {
                            results.push(entry);
                        }
                        if let Some(l) = self.limit {
                            if results.len() >= l { break; }
                        }
                    }
                }
            },
            CurrentMode::Ultra(um) => {
                let all = um.localstore.all_entries();
                for (k, v) in all {
                    if !k.starts_with(&self.prefix) { continue; }
                    if let Ok(entry) = bincode::deserialize::<LocalStoreEntry>(&v) {
                        if self.filters.iter().all(|f| f(&entry)) {
                            if let Some(val) = &self.value_eq {
                                if entry.value != *val { continue; }
                            }
                            results.push(entry);
                        }
                    }
                    if let Some(l) = self.limit {
                        if results.len() >= l { break; }
                    }
                }
            }
        };

        Ok(results)
    }
}

pub struct BinaryStoreTable<'a> { container: &'a Container }
impl<'a> BinaryStoreTable<'a> {
    pub fn count(&self) -> Result<usize, Box<dyn std::error::Error>> {
        match &*self.container.switcher.current_mode.read() {
            CurrentMode::Persistent(pm) => Ok(pm.binarystore.all_entries().len()),
            CurrentMode::Ultra(um) => Ok(um.binarystore.entry_count.load(std::sync::atomic::Ordering::SeqCst)),
        }
    }
    pub fn put(&self, key: Vec<u8>, value: Vec<u8>) -> Result<(), Box<dyn std::error::Error>> {
        match &*self.container.switcher.current_mode.read() {
            CurrentMode::Persistent(pm) => pm.binarystore.put(key, value)?,
            CurrentMode::Ultra(um) => um.binarystore.put(key, value, 0),
        }
        Ok(())
    }
    pub fn get(&self, key: &[u8]) -> Result<Option<Vec<u8>>, Box<dyn std::error::Error>> {
        let value_opt = match &*self.container.switcher.current_mode.read() {
            CurrentMode::Persistent(pm) => pm.binarystore.get(key).map(|e| e.value),
            CurrentMode::Ultra(um) => um.binarystore.get(key),
        };
        Ok(value_opt)
    }
    pub fn delete(&self, key: &[u8]) -> Result<(), Box<dyn std::error::Error>> {
        match &*self.container.switcher.current_mode.read() {
            CurrentMode::Persistent(pm) => pm.binarystore.delete(key.to_vec())?,
            CurrentMode::Ultra(um) => um.binarystore.delete(key),
        }
        Ok(())
    }
    pub fn scan_prefix(&self, prefix: &[u8]) -> Result<Vec<(Vec<u8>, Vec<u8>)>, Box<dyn std::error::Error>> {
        let entries = match &*self.container.switcher.current_mode.read() {
            CurrentMode::Persistent(pm) => {
                pm.binarystore.scan_prefix(prefix).into_iter().map(|e| (e.key, e.value)).collect()
            },
            CurrentMode::Ultra(um) => {
                um.binarystore.all_entries().into_iter()
                    .filter(|(k, _)| k.starts_with(prefix))
                    .collect()
            }
        };
        Ok(entries)
    }
    pub fn all_entries(&self) -> Result<Vec<(Vec<u8>, Vec<u8>)>, Box<dyn std::error::Error>> {
        let entries = match &*self.container.switcher.current_mode.read() {
            CurrentMode::Persistent(pm) => {
                pm.binarystore.all_entries().into_iter().map(|e| (e.key, e.value)).collect()
            },
            CurrentMode::Ultra(um) => um.binarystore.all_entries(),
        };
        Ok(entries)
    }
}

pub struct SettingsTable<'a> { container: &'a Container }
impl<'a> SettingsTable<'a> {
    pub fn count(&self) -> Result<usize, Box<dyn std::error::Error>> {
        match &*self.container.switcher.current_mode.read() {
            CurrentMode::Persistent(pm) => Ok(pm.settings.all_entries().len()),
            CurrentMode::Ultra(um) => Ok(um.settings.entry_count.load(std::sync::atomic::Ordering::SeqCst)),
        }
    }
    pub fn set(&self, key: &str, value: &str) -> Result<(), Box<dyn std::error::Error>> {
        let k = key.as_bytes().to_vec();
        let v = value.as_bytes().to_vec();
        match &*self.container.switcher.current_mode.read() {
            CurrentMode::Persistent(pm) => pm.settings.put(k, v)?,
            CurrentMode::Ultra(um) => um.settings.put(k, v, 0),
        }
        Ok(())
    }
    
    pub fn get(&self, key: &str) -> Result<Option<String>, Box<dyn std::error::Error>> {
        let k = key.as_bytes();
        let value_opt = match &*self.container.switcher.current_mode.read() {
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
