//! BrowserDB Rust FFI bindings
//! 
//! This crate provides high-level Rust APIs for BrowserDB operations,
//! bridging to the high-performance Zig core engine.

use std::sync::Arc;
use parking_lot::RwLock;

mod ffi;
pub mod types;
pub mod operations;

/// BrowserDB主结构体
pub struct BrowserDB {
    inner: Arc<RwLock<BrowserDBInner>>,
}

struct BrowserDBInner {
    // 数据库路径
    path: String,
    // 模式: Persistent 或 Ultra
    mode: DatabaseMode,
    // 内部状态
    initialized: bool,
}

#[derive(Clone, Copy)]
pub enum DatabaseMode {
    Persistent, // 持久化模式 - 数据写入磁盘
    Ultra,      // 超快模式 - 全内存，数据不持久化
}

/// 历史记录条目
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct HistoryEntry {
    pub timestamp: u64,
    pub url_hash: u128,
    pub title: String,
    pub visit_count: u32,
}

/// Cookie条目
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct CookieEntry {
    pub domain_hash: u128,
    pub name: String,
    pub value: String,
    pub expiry: u64,
    pub flags: u8, // secure/httponly等标记
}

/// 缓存条目
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct CacheEntry {
    pub url_hash: u128,
    pub headers: String,
    pub body: Vec<u8>,
    pub etag: String,
    pub last_modified: u64,
}

/// 本地存储条目
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct LocalStoreEntry {
    pub origin_hash: u128,
    pub key: String,
    pub value: String,
}

/// 设置条目
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SettingEntry {
    pub key: String,
    pub value: String,
}

impl BrowserDB {
    /// 打开或创建数据库
    pub fn open<P: Into<String>>(path: P) -> Result<Self, Box<dyn std::error::Error>> {
        let path = path.into();
        
        let inner = Arc::new(RwLock::new(BrowserDBInner {
            path: path.clone(),
            mode: DatabaseMode::Persistent,
            initialized: false,
        }));

        // 初始化Zig核心引擎
        unsafe {
            ffi::browserdb_init(path.as_ptr(), path.len())?;
        }

        Ok(BrowserDB { inner })
    }

    /// 切换数据库模式
    pub fn set_mode(&self, mode: DatabaseMode) -> Result<(), Box<dyn std::error::Error>> {
        let mut inner = self.inner.write();
        inner.mode = mode;

        // TODO: 实现模式切换的Zig调用
        Ok(())
    }

    /// 获取数据库统计信息
    pub fn stats(&self) -> Result<DatabaseStats, Box<dyn std::error::Error>> {
        // TODO: 实现统计信息获取
        Ok(DatabaseStats {
            total_entries: 0,
            history_entries: 0,
            cookie_entries: 0,
            cache_entries: 0,
            memory_usage_mb: 0,
            disk_usage_mb: 0,
        })
    }

    /// 完全清理数据库
    pub fn wipe(&self) -> Result<(), Box<dyn std::error::Error>> {
        let inner = self.inner.read();
        unsafe {
            ffi::browserdb_wipe(inner.path.as_ptr(), inner.path.len())?;
        }
        Ok(())
    }

    /// 获取历史记录表的操作接口
    pub fn history(&self) -> HistoryTable {
        HistoryTable { db: self }
    }

    /// 获取Cookie表的操作接口
    pub fn cookies(&self) -> CookiesTable {
        CookiesTable { db: self }
    }

    /// 获取缓存表的操作接口
    pub fn cache(&self) -> CacheTable {
        CacheTable { db: self }
    }

    /// 获取本地存储表的操作接口
    pub fn localstore(&self) -> LocalStoreTable {
        LocalStoreTable { db: self }
    }

    /// 获取设置表的操作接口
    pub fn settings(&self) -> SettingsTable {
        SettingsTable { db: self }
    }
}

impl Drop for BrowserDB {
    fn drop(&mut self) {
        // 清理Zig资源
        unsafe {
            ffi::browserdb_cleanup();
        }
    }
}

/// 数据库统计信息
#[derive(Debug, Clone)]
pub struct DatabaseStats {
    pub total_entries: u64,
    pub history_entries: u64,
    pub cookie_entries: u64,
    pub cache_entries: u64,
    pub memory_usage_mb: u64,
    pub disk_usage_mb: u64,
}

/// 历史记录操作表
pub struct HistoryTable<'a> {
    db: &'a BrowserDB,
}

impl<'a> HistoryTable<'a> {
    /// 插入历史记录
    pub fn insert(&self, entry: &HistoryEntry) -> Result<(), Box<dyn std::error::Error>> {
        // TODO: 实现FFI调用到Zig核心
        Ok(())
    }

    /// 获取历史记录
    pub fn get(&self, url_hash: u128) -> Result<Option<HistoryEntry>, Box<dyn std::error::Error>> {
        // TODO: 实现FFI调用到Zig核心
        Ok(None)
    }

    /// 热查询 - 基于热度排序
    pub fn hot_search(&self, query: &str, limit: usize) -> Result<Vec<HistoryEntry>, Box<dyn std::error::Error>> {
        // TODO: 实现热查询（基于HeatMap）
        Ok(Vec::new())
    }

    /// 搜索历史记录
    pub fn search(&self, query: &str, limit: usize) -> Result<Vec<HistoryEntry>, Box<dyn std::error::Error>> {
        // TODO: 实现搜索
        Ok(Vec::new())
    }

    /// 删除特定域名的所有记录
    pub fn wipe_domain(&self, domain: &str) -> Result<(), Box<dyn std::error::Error>> {
        // TODO: 实现域名清理（GDPR合规）
        Ok(())
    }
}

/// Cookie操作表
pub struct CookiesTable<'a> {
    db: &'a BrowserDB,
}

impl<'a> CookiesTable<'a> {
    /// 插入Cookie
    pub fn insert(&self, cookie: &CookieEntry) -> Result<(), Box<dyn std::error::Error>> {
        Ok(())
    }

    /// 获取域名的Cookie
    pub fn get_by_domain(&self, domain_hash: u128) -> Result<Vec<CookieEntry>, Box<dyn std::error::Error>> {
        Ok(Vec::new())
    }
}

/// Cache操作表
pub struct CacheTable<'a> {
    db: &'a BrowserDB,
}

impl<'a> CacheTable<'a> {
    /// 插入缓存条目
    pub fn insert(&self, entry: &CacheEntry) -> Result<(), Box<dyn std::error::Error>> {
        Ok(())
    }

    /// 基于URL哈希获取缓存
    pub fn get(&self, url_hash: u128) -> Result<Option<CacheEntry>, Box<dyn std::error::Error>> {
        Ok(None)
    }

    /// 清理低热度条目
    pub fn evict_heat(&self, min_heat: f32) -> Result<u64, Box<dyn std::error::Error>> {
        // TODO: 实现基于热度的清理策略
        Ok(0)
    }
}

/// LocalStore操作表
pub struct LocalStoreTable<'a> {
    db: &'a BrowserDB,
}

impl<'a> LocalStoreTable<'a> {
    /// 插入本地存储条目
    pub fn insert(&self, entry: &LocalStoreEntry) -> Result<(), Box<dyn std::error::Error>> {
        Ok(())
    }

    /// 获取特定源的数据
    pub fn get_by_origin(&self, origin_hash: u128) -> Result<Vec<LocalStoreEntry>, Box<dyn std::error::Error>> {
        Ok(Vec::new())
    }
}

/// Settings操作表
pub struct SettingsTable<'a> {
    db: &'a BrowserDB,
}

impl<'a> SettingsTable<'a> {
    /// 设置配置项
    pub fn set(&self, key: &str, value: &str) -> Result<(), Box<dyn std::error::Error>> {
        Ok(())
    }

    /// 获取配置项
    pub fn get(&self, key: &str) -> Result<Option<String>, Box<dyn std::error::Error>> {
        Ok(None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_database_creation() {
        let temp_dir = tempdir().unwrap();
        let db_path = temp_dir.path().join("test.bdb");
        
        let db = BrowserDB::open(db_path.to_str().unwrap()).unwrap();
        assert!(db.inner.read().initialized);
    }

    #[test]
    fn test_mode_switching() {
        let temp_dir = tempdir().unwrap();
        let db_path = temp_dir.path().join("test.bdb");
        
        let db = BrowserDB::open(db_path.to_str().unwrap()).unwrap();
        db.set_mode(DatabaseMode::Ultra).unwrap();
        
        // 验证模式切换（TODO: 实际实现）
        assert_eq!(db.inner.read().mode, DatabaseMode::Ultra);
    }
}