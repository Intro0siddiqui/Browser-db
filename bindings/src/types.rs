//! BrowserDB数据类型定义
//! 
//! 这个模块定义了所有在BrowserDB中使用的核心数据类型。

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// 数据库模式枚举
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum DatabaseMode {
    /// 持久化模式 - 数据写入磁盘，支持崩溃恢复
    Persistent,
    /// 超快模式 - 所有数据驻留在内存中，重启后清空
    Ultra,
}

/// 表类型枚举
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum TableType {
    History = 1,
    Cookies = 2,
    Cache = 3,
    LocalStore = 4,
    Settings = 5,
}

/// 热度分数 (0.0 - 1.0)
/// 
/// 用于HeatMap索引系统的动态热度跟踪
/// - 1.0: 非常热门，经常访问
/// - 0.5: 中等热度，偶尔访问
/// - 0.0: 冷数据，几乎不访问
pub type Heat = f32;

/// 历史记录条目
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HistoryEntry {
    /// Unix时间戳（毫秒）
    pub timestamp: u64,
    /// URL的Blake3哈希值
    pub url_hash: u128,
    /// 页面标题
    pub title: String,
    /// 访问次数
    pub visit_count: u32,
}

/// Cookie条目
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CookieEntry {
    /// 域名哈希值
    pub domain_hash: u128,
    /// Cookie名称
    pub name: String,
    /// Cookie值
    pub value: String,
    /// 过期时间（Unix时间戳，毫秒）
    pub expiry: u64,
    /// 标志位（secure/httponly等）
    pub flags: u8,
}

/// 缓存条目
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CacheEntry {
    /// URL哈希值
    pub url_hash: u128,
    /// HTTP响应头
    pub headers: String,
    /// 响应体数据
    pub body: Vec<u8>,
    /// ETag值
    pub etag: String,
    /// Last-Modified时间戳
    pub last_modified: u64,
}

/// 本地存储条目
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LocalStoreEntry {
    /// 源地址哈希值
    pub origin_hash: u128,
    /// 存储键名
    pub key: String,
    /// 存储值（JSON字符串）
    pub value: String,
}

/// 设置条目
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SettingEntry {
    /// 设置键名
    pub key: String,
    /// 设置值（JSON字符串）
    pub value: String,
}

/// 数据库统计信息
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DatabaseStats {
    pub total_entries: u64,
    pub history_entries: u64,
    pub cookie_entries: u64,
    pub cache_entries: u64,
    pub localstore_entries: u64,
    pub settings_entries: u64,
    pub memory_usage_mb: u64,
    pub disk_usage_mb: u64,
    pub average_query_time_ms: f64,
    pub cache_hit_rate: f32,
}

/// HeatMap统计信息
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HeatMapStats {
    pub hot_entries: u64,
    pub warm_entries: u64,
    pub cold_entries: u64,
    pub average_heat: f32,
    pub heat_decay_rate: f32,
}

/// 查询结果
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueryResult<T> {
    pub results: Vec<T>,
    pub total_found: u64,
    pub query_time_ms: f64,
    pub cache_hit: bool,
}

/// 性能基准测试结果
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BenchmarkResult {
    pub write_ops_per_sec: u64,
    pub read_ops_per_sec: u64,
    pub hot_query_ops_per_sec: u64,
    pub average_write_latency_ms: f64,
    pub average_read_latency_ms: f64,
    pub average_hot_query_latency_ms: f64,
    pub memory_footprint_mb: u64,
    pub disk_footprint_mb: u64,
}

/// Cookie标志位
pub mod cookie_flags {
    pub const SECURE: u8 = 0b00000001;
    pub const HTTP_ONLY: u8 = 0b00000010;
    pub const SAME_SITE: u8 = 0b00000100;
    pub const PERSISTENT: u8 = 0b00001000;
}

/// HeatMap阈值
pub mod heat_thresholds {
    pub const HOT_THRESHOLD: f32 = 0.7;
    pub const WARM_THRESHOLD: f32 = 0.3;
    pub const COLD_THRESHOLD: f32 = 0.1;
    
    pub fn categorize_heat(heat: f32) -> &'static str {
        if heat >= HOT_THRESHOLD {
            "hot"
        } else if heat >= WARM_THRESHOLD {
            "warm"
        } else if heat >= COLD_THRESHOLD {
            "cold"
        } else {
            "frozen"
        }
    }
}

/// LSM-Tree配置参数
pub mod lsm_config {
    /// MemTable大小阈值（字节）
    pub const MEMTABLE_SIZE_BYTES: usize = 4 * 1024 * 1024; // 4MB
    /// MemTable操作阈值
    pub const MEMTABLE_OPS_THRESHOLD: usize = 1000;
    /// SSTable最小大小（字节）
    pub const SSTABLE_MIN_SIZE_BYTES: usize = 64 * 1024 * 1024; // 64MB
    /// Bloom过滤器期望的假阳性率
    pub const BLOOM_FPR: f32 = 0.01;
    /// 压缩任务的CPU使用率限制
    pub const COMPACTION_CPU_LIMIT: f32 = 0.05; // 5%
}

/// 工具函数
impl HistoryEntry {
    /// 创建新的历史记录
    pub fn new(url_hash: u128, title: String) -> Self {
        Self {
            timestamp: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_millis() as u64,
            url_hash,
            title,
            visit_count: 1,
        }
    }

    /// 增加访问次数
    pub fn increment_visit(&mut self) {
        self.visit_count += 1;
        self.timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64;
    }
}

impl CookieEntry {
    /// 创建新的Cookie
    pub fn new(
        domain_hash: u128,
        name: String,
        value: String,
        expiry: u64,
    ) -> Self {
        Self {
            domain_hash,
            name,
            value,
            expiry,
            flags: 0,
        }
    }

    /// 设置安全标志
    pub fn set_secure(&mut self) {
        self.flags |= cookie_flags::SECURE;
    }

    /// 设置HttpOnly标志
    pub fn set_httponly(&mut self) {
        self.flags |= cookie_flags::HTTP_ONLY;
    }

    /// 检查是否为安全Cookie
    pub fn is_secure(&self) -> bool {
        self.flags & cookie_flags::SECURE != 0
    }

    /// 检查是否为HttpOnly Cookie
    pub fn is_httponly(&self) -> bool {
        self.flags & cookie_flags::HTTP_ONLY != 0
    }
}

impl SettingEntry {
    /// 创建设置条目
    pub fn new(key: String, value: String) -> Self {
        Self { key, value }
    }
}