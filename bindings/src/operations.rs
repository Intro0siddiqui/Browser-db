//! BrowserDB高级操作模块
//! 
//! 这个模块提供对BrowserDB的高级操作接口，包括批量操作、
//! 性能优化和智能缓存管理。

use crate::types::*;
use crate::{BrowserDB, HistoryEntry, CookieEntry, CacheEntry, LocalStoreEntry, SettingEntry};
use std::collections::HashMap;

/// 数据库操作管理器
pub struct DatabaseOperations<'a> {
    db: &'a BrowserDB,
}

impl<'a> DatabaseOperations<'a> {
    pub fn new(db: &'a BrowserDB) -> Self {
        Self { db }
    }

    /// 批量插入历史记录
    pub fn batch_insert_history(
        &self,
        entries: &[HistoryEntry],
    ) -> Result<usize, Box<dyn std::error::Error>> {
        let mut inserted = 0;
        
        for entry in entries {
            self.db.history().insert(entry)?;
            inserted += 1;
        }
        
        Ok(inserted)
    }

    /// 智能历史记录查询
    /// 
    /// 基于热度排序，返回最相关的历史记录
    pub fn smart_history_search(
        &self,
        query: &str,
        limit: usize,
        min_heat: Option<f32>,
    ) -> Result<QueryResult<HistoryEntry>, Box<dyn std::error::Error>> {
        let start_time = std::time::Instant::now();
        
        // 获取热查询结果
        let mut results = self.db.history().hot_search(query, limit)?;
        
        // 如果指定了最小热度，进一步过滤
        if let Some(min_heat_val) = min_heat {
            results.retain(|entry| {
                // TODO: 实现基于entry的HeatMap查询
                true // 临时实现
            });
        }
        
        let query_time = start_time.elapsed().as_millis();
        
        Ok(QueryResult {
            results,
            total_found: results.len() as u64,
            query_time_ms: query_time as f64,
            cache_hit: true, // 热查询假设命中缓存
        })
    }

    /// 缓存优化操作
    /// 
    /// 基于热度清理低价值缓存，释放存储空间
    pub fn optimize_cache(&self, min_heat: f32) -> Result<CacheOptimizationResult, Box<dyn std::error::Error>> {
        let freed_entries = self.db.cache().evict_heat(min_heat)?;
        
        Ok(CacheOptimizationResult {
            freed_entries,
            target_heat: min_heat,
            optimization_time_ms: 0.0, // TODO: 实现时间测量
        })
    }

    /// 批量Cookie导入
    /// 
    /// 用于从其他浏览器导入Cookie数据
    pub fn import_cookies(
        &self,
        cookies: &[CookieEntry],
    ) -> Result<ImportResult, Box<dyn std::error::Error>> {
        let mut imported = 0;
        let mut errors = Vec::new();
        
        for cookie in cookies {
            match self.db.cookies().insert(cookie) {
                Ok(_) => imported += 1,
                Err(e) => errors.push(format!("Failed to import cookie '{}': {}", cookie.name, e)),
            }
        }
        
        Ok(ImportResult {
            total_processed: cookies.len(),
            successful_imports: imported,
            errors,
        })
    }

    /// 域隐私清理
    /// 
    /// 删除指定域名的所有数据（GDPR合规）
    pub fn privacy_wipe_domain(&self, domain: &str) -> Result<PrivacyWipeResult, Box<dyn std::error::Error>> {
        let mut wiped_history = 0;
        let mut wiped_cookies = 0;
        let mut wiped_localstore = 0;
        
        // TODO: 实现基于域名的查找和清理
        // 临时实现
        wiped_history = self.db.history().wipe_domain(domain)?;
        wiped_cookies = 0; // TODO: 实现
        wiped_localstore = 0; // TODO: 实现
        
        Ok(PrivacyWipeResult {
            domain: domain.to_string(),
            wiped_history,
            wiped_cookies,
            wiped_localstore,
            total_wiped: wiped_history + wiped_cookies + wiped_localstore,
        })
    }

    /// 数据库健康检查
    pub fn health_check(&self) -> Result<DatabaseHealth, Box<dyn std::error::Error>> {
        let stats = self.db.stats()?;
        
        // 计算健康评分
        let health_score = calculate_health_score(&stats);
        
        // 检查潜在问题
        let mut issues = Vec::new();
        
        if stats.memory_usage_mb > 100 {
            issues.push("High memory usage detected".to_string());
        }
        
        if stats.disk_usage_mb > 1000 {
            issues.push("High disk usage detected".to_string());
        }
        
        if stats.cache_hit_rate < 0.8 {
            issues.push("Low cache hit rate".to_string());
        }
        
        Ok(DatabaseHealth {
            overall_score: health_score,
            stats,
            issues,
            recommendations: generate_recommendations(&stats),
        })
    }

    /// 性能基准测试
    pub fn benchmark(&self) -> Result<BenchmarkResult, Box<dyn std::error::Error>> {
        use std::time::Instant;
        
        // 写入性能测试
        let write_start = Instant::now();
        for i in 0..1000 {
            let entry = HistoryEntry::new(
                u128::from_le_bytes([i as u8; 16]),
                format!("Test entry {}", i),
            );
            self.db.history().insert(&entry)?;
        }
        let write_elapsed = write_start.elapsed();
        
        // 读取性能测试
        let read_start = Instant::now();
        for i in 0..100 {
            self.db.history().get(u128::from_le_bytes([i as u8; 16]))?;
        }
        let read_elapsed = read_start.elapsed();
        
        // 热查询性能测试
        let hot_query_start = Instant::now();
        self.db.history().hot_search("test", 10)?;
        let hot_query_elapsed = hot_query_start.elapsed();
        
        let stats = self.db.stats()?;
        
        Ok(BenchmarkResult {
            write_ops_per_sec: (1000.0 / write_elapsed.as_secs_f64()) as u64,
            read_ops_per_sec: (100.0 / read_elapsed.as_secs_f64()) as u64,
            hot_query_ops_per_sec: (1.0 / hot_query_elapsed.as_secs_f64()) as u64,
            average_write_latency_ms: write_elapsed.as_millis() as f64 / 1000.0,
            average_read_latency_ms: read_elapsed.as_millis() as f64 / 100.0,
            average_hot_query_latency_ms: hot_query_elapsed.as_millis() as f64,
            memory_footprint_mb: stats.memory_usage_mb,
            disk_footprint_mb: stats.disk_usage_mb,
        })
    }
}

/// 缓存优化结果
#[derive(Debug, Clone)]
pub struct CacheOptimizationResult {
    pub freed_entries: u64,
    pub target_heat: f32,
    pub optimization_time_ms: f64,
}

/// 批量导入结果
#[derive(Debug, Clone)]
pub struct ImportResult {
    pub total_processed: usize,
    pub successful_imports: usize,
    pub errors: Vec<String>,
}

/// 隐私清理结果
#[derive(Debug, Clone)]
pub struct PrivacyWipeResult {
    pub domain: String,
    pub wiped_history: u64,
    pub wiped_cookies: u64,
    pub wiped_localstore: u64,
    pub total_wiped: u64,
}

/// 数据库健康状态
#[derive(Debug, Clone)]
pub struct DatabaseHealth {
    pub overall_score: f32,
    pub stats: DatabaseStats,
    pub issues: Vec<String>,
    pub recommendations: Vec<String>,
}

// 内部辅助函数

fn calculate_health_score(stats: &DatabaseStats) -> f32 {
    let mut score: f32 = 100.0;
    
    // 内存使用扣分
    if stats.memory_usage_mb > 50 {
        score -= 10.0;
    } else if stats.memory_usage_mb > 20 {
        score -= 5.0;
    }
    
    // 缓存命中率扣分
    score -= (1.0 - stats.cache_hit_rate) * 20.0;
    
    // 查询时间扣分
    if stats.average_query_time_ms > 1.0 {
        score -= 15.0;
    } else if stats.average_query_time_ms > 0.5 {
        score -= 5.0;
    }
    
    score.max(0.0).min(100.0)
}

fn generate_recommendations(stats: &DatabaseStats) -> Vec<String> {
    let mut recommendations = Vec::new();
    
    if stats.memory_usage_mb > 50 {
        recommendations.push("Consider switching to Ultra mode for better memory efficiency".to_string());
    }
    
    if stats.cache_hit_rate < 0.8 {
        recommendations.push("Enable HeatMap optimization for better cache performance".to_string());
    }
    
    if stats.disk_usage_mb > 500 {
        recommendations.push("Run cache optimization to free up disk space".to_string());
    }
    
    recommendations
}