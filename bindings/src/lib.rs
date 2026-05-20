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

pub struct Container {
    pub name: String,
    pub switcher: Arc<ModeSwitcher>,
    pub pku: u16, // Hardware Protection Key for Hajr HAL
}

impl Container {
    pub fn history(&self) -> HistoryTable<'_> { HistoryTable { container: self } }
    pub fn cookies(&self) -> CookiesTable<'_> { CookiesTable { container: self } }
    pub fn cache(&self) -> CacheTable<'_> { CacheTable { container: self } }
    pub fn localstore(&self) -> LocalStoreTable<'_> { LocalStoreTable { container: self } }
    pub fn settings(&self) -> SettingsTable<'_> { SettingsTable { container: self } }

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
                extractor: Arc::new(|_k, v| {
                    if let Ok(entry) = bincode::deserialize::<LocalStoreEntry>(v) {
                        Some(format!("idx:localstore:value:{}:{}", entry.value, entry.key).into_bytes())
                    } else {
                        None
                    }
                }),
            }
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

    pub fn set_mode(&self, mode: DatabaseMode) -> Result<(), Box<dyn std::error::Error>> {
        self.container("default").unwrap().set_mode(mode)
    }

    pub fn stats(&self) -> Result<DatabaseStats, Box<dyn std::error::Error>> {
        self.container("default").unwrap().stats()
    }
    
    pub fn wipe(&self) -> Result<(), Box<dyn std::error::Error>> {
        self.container("default").unwrap().wipe()
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

pub struct HistoryTable<'a> { container: &'a Container }
impl<'a> HistoryTable<'a> {
    pub fn count(&self) -> Result<usize, Box<dyn std::error::Error>> {
        match &*self.container.switcher.current_mode.read() {
            CurrentMode::Persistent(pm) => Ok(pm.history.all_entries().len()),
            CurrentMode::Ultra(um) => Ok(um.history.entry_count.load(std::sync::atomic::Ordering::SeqCst)),
        }
    }

    pub fn insert(&self, entry: &HistoryEntry) -> Result<(), Box<dyn std::error::Error>> {
        let key = bincode::serialize(&entry.url_hash)?;
        let value = bincode::serialize(entry)?;
        
        match &*self.container.switcher.current_mode.read() {
            CurrentMode::Persistent(pm) => pm.history.put(key, value)?,
            CurrentMode::Ultra(um) => um.history.put(key, value),
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

    pub fn hot_search(&self, _query: &str, _limit: usize) -> Result<Vec<HistoryEntry>, Box<dyn std::error::Error>> {
        Ok(Vec::new())
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
            CurrentMode::Ultra(um) => um.cookies.put(key, value),
        }
        Ok(())
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
            CurrentMode::Ultra(um) => um.cache.put(key, value),
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
                um.localstore.put(primary_key.clone(), value.clone());
                // Ultra mode still needs manual indexing for now or update it as well
                let idx_key = format!("idx:localstore:value:{}:{}", entry.value, entry.key);
                um.localstore.put(idx_key.into_bytes(), primary_key);
            }
        }
        Ok(())
    }

    pub fn insert_with_index(&self, entry: &LocalStoreEntry, _index_fields: &[&str]) -> Result<(), Box<dyn std::error::Error>> {
        self.insert(entry)
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
            CurrentMode::Ultra(um) => um.settings.put(k, v),
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
