use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use parking_lot::RwLock;

use crate::core::lsm_tree::LSMTree;
use crate::core::format::TableType;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DatabaseMode {
    Persistent,
    Ultra,
}

#[derive(Debug, Clone)]
pub struct ModeConfig {
    pub max_memory: usize,
    pub enable_compression: bool,
    pub enable_heat_tracking: bool,
}

pub struct UltraTable {
    pub data: RwLock<HashMap<Vec<u8>, Vec<u8>>>,
}

impl UltraTable {
    pub fn new() -> Self {
        Self {
            data: RwLock::new(HashMap::new()),
        }
    }

    pub fn put(&self, key: Vec<u8>, value: Vec<u8>) {
        self.data.write().insert(key, value);
    }

    pub fn get(&self, key: &[u8]) -> Option<Vec<u8>> {
        self.data.read().get(key).cloned()
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
        }
    }
    
    pub fn switch_mode(&self, new_mode: DatabaseMode, path: &Path) {
        let mut current = self.current_mode.write();
        
        // Data Migration Logic (Simplified)
        // In a real app, we would iterate all data from old mode and put into new mode.
        // For now, we just initialize the new mode.
        
        *current = match new_mode {
            DatabaseMode::Persistent => CurrentMode::Persistent(PersistentMode::new(path, &self.config)),
            DatabaseMode::Ultra => CurrentMode::Ultra(UltraMode::new()),
        };
    }
}
