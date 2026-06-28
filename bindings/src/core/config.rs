use serde::Deserialize;
use std::fs;
use std::path::Path;

#[derive(Debug, Deserialize, Clone)]
pub struct LsmTreeConfig {
    pub max_level0_files: usize,
    pub max_memtable_size_mb: usize,
    pub level_size_thresholds_mb: Vec<usize>,
    #[serde(default = "default_compaction_cpu_limit")]
    pub compaction_cpu_limit: f64,
    #[serde(default = "default_compaction_idle_threshold_ms")]
    pub compaction_idle_threshold_ms: u64,
    #[serde(default = "default_compaction_deadline_sec")]
    pub compaction_deadline_sec: u64,
    #[serde(default)]
    pub verify_checksums: bool,
}

fn default_compaction_cpu_limit() -> f64 {
    0.05
}

fn default_compaction_idle_threshold_ms() -> u64 {
    5000
}

fn default_compaction_deadline_sec() -> u64 {
    30
}

impl Default for LsmTreeConfig {
    fn default() -> Self {
        Self {
            max_level0_files: 4,
            max_memtable_size_mb: 20,
            level_size_thresholds_mb: vec![10, 100, 1000, 10000, 100000, 1000000],
            compaction_cpu_limit: 0.05,
            compaction_idle_threshold_ms: 5000,
            compaction_deadline_sec: 30,
            verify_checksums: false,
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
