use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use parking_lot::RwLock;

use crate::core::lsm_tree::LSMTree;
use crate::core::format::TableType;

use std::fmt;

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
}

pub struct UltraTable {
    pub data: RwLock<HashMap<Vec<u8>, Vec<u8>>>,
    pub entry_count: std::sync::atomic::AtomicUsize,
}

impl UltraTable {
    pub fn new() -> Self {
        Self {
            data: RwLock::new(HashMap::new()),
            entry_count: std::sync::atomic::AtomicUsize::new(0),
        }
    }

    pub fn put(&self, key: Vec<u8>, value: Vec<u8>) {
        if self.data.write().insert(key, value).is_none() {
            self.entry_count.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        }
    }

    pub fn get(&self, key: &[u8]) -> Option<Vec<u8>> {
        self.data.read().get(key).cloned()
    }

    pub fn delete(&self, key: &[u8]) {
        if self.data.write().remove(key).is_some() {
            self.entry_count.fetch_sub(1, std::sync::atomic::Ordering::SeqCst);
        }
    }

    pub fn all_entries(&self) -> Vec<(Vec<u8>, Vec<u8>)> {
        self.data.read().iter().map(|(k, v)| (k.clone(), v.clone())).collect()
    }
}

pub struct PersistentMode {
    pub path: PathBuf,
    pub history: LSMTree,
    pub cookies: LSMTree,
    pub cache: LSMTree,
    pub localstore: LSMTree,
    pub settings: LSMTree,
}

impl PersistentMode {
    pub fn has_unsynced_data(&self) -> bool {
        !self.history.memtable.read().entries.is_empty() ||
        !self.cookies.memtable.read().entries.is_empty() ||
        !self.cache.memtable.read().entries.is_empty() ||
        !self.localstore.memtable.read().entries.is_empty() ||
        !self.settings.memtable.read().entries.is_empty()
    }

    pub fn new(path: &Path, config: &ModeConfig) -> Self {
        let max_mem = config.max_memory / 5; // Divide memory among tables
        Self {
            path: path.to_path_buf(),
            history: LSMTree::new(path, TableType::History, max_mem),
            cookies: LSMTree::new(path, TableType::Cookies, max_mem),
            cache: LSMTree::new(path, TableType::Cache, max_mem),
            localstore: LSMTree::new(path, TableType::LocalStore, max_mem),
            settings: LSMTree::new(path, TableType::Settings, max_mem),
        }
    }
}

pub struct UltraMode {
    pub history: UltraTable,
    pub cookies: UltraTable,
    pub cache: UltraTable,
    pub localstore: UltraTable,
    pub settings: UltraTable,
}

impl UltraMode {
    pub fn new() -> Self {
        Self {
            history: UltraTable::new(),
            cookies: UltraTable::new(),
            cache: UltraTable::new(),
            localstore: UltraTable::new(),
            settings: UltraTable::new(),
        }
    }
}

pub enum CurrentMode {
    Persistent(PersistentMode),
    Ultra(UltraMode),
}

pub struct ModeSwitcher {
    pub current_mode: Arc<RwLock<CurrentMode>>,
    pub config: ModeConfig,
    pub base_path: PathBuf,
}

impl ModeSwitcher {
    pub fn new(path: &Path, mode: DatabaseMode, config: ModeConfig) -> Self {
        let current = match mode {
            DatabaseMode::Persistent => CurrentMode::Persistent(PersistentMode::new(path, &config)),
            DatabaseMode::Ultra => CurrentMode::Ultra(UltraMode::new()),
        };
        
        Self {
            current_mode: Arc::new(RwLock::new(current)),
            config,
            base_path: path.to_path_buf(),
        }
    }
    
    pub fn switch_mode(&self, new_mode: DatabaseMode, path: &Path) -> Result<(), ModeSwitchError> {
        let mut current = self.current_mode.write();
        
        // Check for unsynced data to prevent data loss as per nitpick
        match &*current {
            CurrentMode::Persistent(pm) => {
                if new_mode == DatabaseMode::Ultra && pm.has_unsynced_data() {
                    return Err(ModeSwitchError::DataLoss);
                }
            },
            CurrentMode::Ultra(um) => {
                if new_mode == DatabaseMode::Persistent {
                    // For Ultra mode, "unsynced" is everything if it's not empty
                    if !um.history.data.read().is_empty() ||
                       !um.cookies.data.read().is_empty() ||
                       !um.cache.data.read().is_empty() ||
                       !um.localstore.data.read().is_empty() ||
                       !um.settings.data.read().is_empty() {
                           return Err(ModeSwitchError::DataLoss);
                       }
                }
            }
        }

        *current = match new_mode {
            DatabaseMode::Persistent => CurrentMode::Persistent(PersistentMode::new(path, &self.config)),
            DatabaseMode::Ultra => CurrentMode::Ultra(UltraMode::new()),
        };
        Ok(())
    }
}
