use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use parking_lot::RwLock;

use crate::core::lsm_tree::LSMTree;
use crate::core::format::TableType;

use std::fmt;
use crate::core::config::BrowserDBConfig;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DatabaseMode {
    Persistent,
    Ultra,
}

#[derive(Debug)]
pub enum ModeSwitchError {
    DataLoss,
    IoError(std::io::Error),
}

impl fmt::Display for ModeSwitchError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ModeSwitchError::DataLoss => write!(f, "Potential data loss detected"),
            ModeSwitchError::IoError(e) => write!(f, "IO error: {}", e),
        }
    }
}

impl std::error::Error for ModeSwitchError {}

#[derive(Debug, Clone)]
pub struct ModeConfig {
    pub max_memory: usize,
    pub enable_compression: bool,
    pub enable_heat_tracking: bool,
    pub ext_config: BrowserDBConfig,
}

pub type UltraEntry = (Vec<u8>, u64);

pub struct UltraTable {
    pub data: RwLock<HashMap<Vec<u8>, UltraEntry>>,
    pub entry_count: std::sync::atomic::AtomicUsize,
}

impl Default for UltraTable {
    fn default() -> Self {
        Self {
            data: RwLock::new(HashMap::new()),
            entry_count: std::sync::atomic::AtomicUsize::new(0),
        }
    }
}

#[inline]
fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

impl UltraTable {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn clear(&self) {
        self.data.write().clear();
        self.entry_count.store(0, std::sync::atomic::Ordering::SeqCst);
    }

    /// Insert or replace a key/value. `expires_at` is an absolute UNIX
    /// timestamp in milliseconds; `0` means the entry never expires.
    /// Enforced lazily on read; use [`UltraTable::purge_expired`] to reclaim
    /// memory from expired entries.
    pub fn put(&self, key: Vec<u8>, value: Vec<u8>, expires_at: u64) {
        if self.data.write().insert(key, (value, expires_at)).is_none() {
            self.entry_count.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        }
    }

    pub fn get(&self, key: &[u8]) -> Option<Vec<u8>> {
        let data = self.data.read();
        let (value, expires_at) = data.get(key)?.clone();
        if expires_at != 0 && expires_at < now_ms() {
            return None;
        }
        Some(value)
    }

    pub fn delete(&self, key: &[u8]) {
        if self.data.write().remove(key).is_some() {
            self.entry_count.fetch_sub(1, std::sync::atomic::Ordering::SeqCst);
        }
    }

    pub fn increment(&self, key: &[u8], delta: i64) {
        let mut data = self.data.write();
        let entry = data.entry(key.to_vec());
        match entry {
            std::collections::hash_map::Entry::Occupied(mut occupied) => {
                let (value, expires_at) = occupied.get_mut();
                if value.len() == 8 {
                    let mut arr = [0u8; 8];
                    arr.copy_from_slice(value);
                    let current = i64::from_le_bytes(arr);
                    let new_val = current.wrapping_add(delta);
                    value.copy_from_slice(&new_val.to_le_bytes());
                } else {
                    value.clear();
                    value.extend_from_slice(&delta.to_le_bytes());
                }
                let _ = expires_at;
            }
            std::collections::hash_map::Entry::Vacant(vacant) => {
                vacant.insert((delta.to_le_bytes().to_vec(), 0));
                self.entry_count.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            }
        }
    }

    /// Snapshot of all non-expired entries. Expired entries are filtered
    /// out but not removed from the table; call [`UltraTable::purge_expired`]
    /// to actually reclaim them.
    pub fn all_entries(&self) -> Vec<(Vec<u8>, Vec<u8>)> {
        let now = now_ms();
        self.data
            .read()
            .iter()
            .filter(|(_, (_, expires_at))| *expires_at == 0 || *expires_at >= now)
            .map(|(k, (v, _)): (&Vec<u8>, &UltraEntry)| (k.clone(), v.clone()))
            .collect()
    }

    /// Remove all expired entries and return the number of entries purged.
    /// Use this to free memory in long-lived Ultra mode sessions; no
    /// background thread is spawned (by design — Ultra mode avoids
    /// background work).
    pub fn purge_expired(&self) -> usize {
        let now = now_ms();
        let mut data = self.data.write();
        let before = data.len();
        data.retain(|_, (_, expires_at)| *expires_at == 0 || *expires_at >= now);
        let purged = before - data.len();
        drop(data);
        if purged > 0 {
            self.entry_count.fetch_sub(purged, std::sync::atomic::Ordering::SeqCst);
        }
        purged
    }
}

pub struct PersistentMode {
    pub path: PathBuf,
    pub history: LSMTree,
    pub bookmarks: LSMTree,
    pub cookies: LSMTree,
    pub cache: LSMTree,
    pub localstore: LSMTree,
    pub settings: LSMTree,
}

impl PersistentMode {
    pub fn has_unsynced_data(&self) -> bool {
        self.history.inner.memtable.iter().any(|m| !m.read().entries.is_empty()) ||
        self.bookmarks.inner.memtable.iter().any(|m| !m.read().entries.is_empty()) ||
        self.cookies.inner.memtable.iter().any(|m| !m.read().entries.is_empty()) ||
        self.cache.inner.memtable.iter().any(|m| !m.read().entries.is_empty()) ||
        self.localstore.inner.memtable.iter().any(|m| !m.read().entries.is_empty()) ||
        self.settings.inner.memtable.iter().any(|m| !m.read().entries.is_empty())
    }

    pub fn new(path: &Path, config: &ModeConfig) -> std::io::Result<Self> {
        Self::new_with_indices(path, config, HashMap::new())
    }

    pub fn new_with_indices(
        path: &Path,
        config: &ModeConfig,
        mut index_defs: HashMap<TableType, Vec<crate::core::lsm_tree::IndexDefinition>>
    ) -> std::io::Result<Self> {
        // max_memtable_size_mb dictates the memtable size in bytes
        let max_mem = config.ext_config.lsm_tree.max_memtable_size_mb * 1024 * 1024;
        Ok(Self {
            path: path.to_path_buf(),
            history: LSMTree::new_with_indices(path, TableType::History, max_mem, config.ext_config.clone(), index_defs.remove(&TableType::History).unwrap_or_default())?,
            bookmarks: LSMTree::new_with_indices(path, TableType::Bookmarks, max_mem, config.ext_config.clone(), index_defs.remove(&TableType::Bookmarks).unwrap_or_default())?,
            cookies: LSMTree::new_with_indices(path, TableType::Cookies, max_mem, config.ext_config.clone(), index_defs.remove(&TableType::Cookies).unwrap_or_default())?,
            cache: LSMTree::new_with_indices(path, TableType::Cache, max_mem, config.ext_config.clone(), index_defs.remove(&TableType::Cache).unwrap_or_default())?,
            localstore: LSMTree::new_with_indices(path, TableType::LocalStore, max_mem, config.ext_config.clone(), index_defs.remove(&TableType::LocalStore).unwrap_or_default())?,
            settings: LSMTree::new_with_indices(path, TableType::Settings, max_mem, config.ext_config.clone(), index_defs.remove(&TableType::Settings).unwrap_or_default())?,
        })
    }
}

pub struct UltraMode {
    pub history: UltraTable,
    pub bookmarks: UltraTable,
    pub cookies: UltraTable,
    pub cache: UltraTable,
    pub localstore: UltraTable,
    pub settings: UltraTable,
}

impl Default for UltraMode {
    fn default() -> Self {
        Self {
            history: UltraTable::new(),
            bookmarks: UltraTable::new(),
            cookies: UltraTable::new(),
            cache: UltraTable::new(),
            localstore: UltraTable::new(),
            settings: UltraTable::new(),
        }
    }
}

impl UltraMode {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn clear(&self) {
        self.history.clear();
        self.bookmarks.clear();
        self.cookies.clear();
        self.cache.clear();
        self.localstore.clear();
        self.settings.clear();
    }

    /// Sweep expired entries from every Ultra table. Returns the total
    /// number of entries purged across all tables.
    pub fn purge_expired_all(&self) -> usize {
        self.history.purge_expired()
            + self.bookmarks.purge_expired()
            + self.cookies.purge_expired()
            + self.cache.purge_expired()
            + self.localstore.purge_expired()
            + self.settings.purge_expired()
    }
}

pub enum CurrentMode {
    Persistent(PersistentMode),
    Ultra(Box<UltraMode>),
}

pub struct ModeSwitcher {
    pub current_mode: Arc<RwLock<CurrentMode>>,
    pub config: ModeConfig,
    pub base_path: PathBuf,
}

impl ModeSwitcher {
    pub fn new(path: &Path, mode: DatabaseMode, config: ModeConfig) -> std::io::Result<Self> {
        Self::new_with_indices(path, mode, config, HashMap::new())
    }

    pub fn new_with_indices(
        path: &Path,
        mode: DatabaseMode,
        config: ModeConfig,
        index_defs: HashMap<TableType, Vec<crate::core::lsm_tree::IndexDefinition>>
    ) -> std::io::Result<Self> {
        let current = match mode {
            DatabaseMode::Persistent => CurrentMode::Persistent(PersistentMode::new_with_indices(path, &config, index_defs)?),
            DatabaseMode::Ultra => CurrentMode::Ultra(Box::new(UltraMode::new())),
        };
        
        Ok(Self {
            current_mode: Arc::new(RwLock::new(current)),
            config,
            base_path: path.to_path_buf(),
        })
    }
    
    pub fn switch_mode(&self, new_mode: DatabaseMode, path: &Path) -> Result<(), ModeSwitchError> {
        let mut current = self.current_mode.write();
        
        let new_instance = match new_mode {
            DatabaseMode::Persistent => CurrentMode::Persistent(
                PersistentMode::new(path, &self.config).map_err(ModeSwitchError::IoError)?
            ),
            DatabaseMode::Ultra => CurrentMode::Ultra(Box::new(UltraMode::new())),
        };

        // Data Migration
        match (&*current, &new_instance) {
            (CurrentMode::Persistent(old_pm), CurrentMode::Ultra(new_um)) => {
                for entry in old_pm.history.all_entries() { new_um.history.put(entry.key, entry.value, 0); }
                for entry in old_pm.bookmarks.all_entries() { new_um.bookmarks.put(entry.key, entry.value, 0); }
                for entry in old_pm.cookies.all_entries() { new_um.cookies.put(entry.key, entry.value, 0); }
                for entry in old_pm.cache.all_entries() { new_um.cache.put(entry.key, entry.value, 0); }
                for entry in old_pm.localstore.all_entries() { new_um.localstore.put(entry.key, entry.value, 0); }
                for entry in old_pm.settings.all_entries() { new_um.settings.put(entry.key, entry.value, 0); }
            },
            (CurrentMode::Ultra(old_um), CurrentMode::Persistent(new_pm)) => {
                for (k, v) in old_um.history.all_entries() { new_pm.history.put(k, v).map_err(ModeSwitchError::IoError)?; }
                for (k, v) in old_um.bookmarks.all_entries() { new_pm.bookmarks.put(k, v).map_err(ModeSwitchError::IoError)?; }
                for (k, v) in old_um.cookies.all_entries() { new_pm.cookies.put(k, v).map_err(ModeSwitchError::IoError)?; }
                for (k, v) in old_um.cache.all_entries() { new_pm.cache.put(k, v).map_err(ModeSwitchError::IoError)?; }
                for (k, v) in old_um.localstore.all_entries() { new_pm.localstore.put(k, v).map_err(ModeSwitchError::IoError)?; }
                for (k, v) in old_um.settings.all_entries() { new_pm.settings.put(k, v).map_err(ModeSwitchError::IoError)?; }
            },
            _ => {} // Same mode or unexpected transition
        }

        *current = new_instance;
        Ok(())
    }
}
