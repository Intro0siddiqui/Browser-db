use serde::Deserialize;
use std::fs;
use std::path::Path;
use std::io;

#[derive(Debug, Deserialize, Clone)]
pub struct LsmTreeConfig {
    pub max_level0_files: usize,
    pub max_memtable_size_mb: usize,
}

impl Default for LsmTreeConfig {
    fn default() -> Self {
        Self {
            max_level0_files: 4,
            max_memtable_size_mb: 20,
        }
    }
}

#[derive(Debug, Deserialize, Clone)]
pub struct HeatmapConfig {
    pub max_entries: usize,
    pub hot_threshold: u32,
    pub decay_factor: f64,
}

impl Default for HeatmapConfig {
    fn default() -> Self {
        Self {
            max_entries: 10000,
            hot_threshold: 10,
            decay_factor: 0.95,
        }
    }
}

#[derive(Debug, Deserialize, Default, Clone)]
pub struct BrowserDBConfig {
    #[serde(default)]
    pub lsm_tree: LsmTreeConfig,
    #[serde(default)]
    pub heatmap: HeatmapConfig,
}

impl BrowserDBConfig {
    pub fn load_or_default(base_path: &Path) -> Self {
        let config_path = base_path.join("browserdb.toml");
        if let Ok(content) = fs::read_to_string(&config_path) {
            toml::from_str(&content).unwrap_or_default()
        } else {
            Self::default()
        }
    }
}
