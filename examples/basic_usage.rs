//! BrowserDB 基础使用示例
//! 
//! 这个文件展示了如何使用BrowserDB进行基本的数据库操作。

use browserdb::*;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // 初始化日志
    tracing_subscriber::fmt::init();
    
    println!("🚀 BrowserDB 基础使用示例");
    println!("============================\n");
    
    // 1. 创建数据库
    let db = BrowserDB::open("/tmp/example.bdb")?;
    println!("✅ 数据库已创建");
    
    // 2. 基本历史记录操作
    println!("\n📚 历史记录操作示例:");
    
    let history_entry = HistoryEntry {
        timestamp: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_millis(),
        url_hash: 0x123456789abcdef0,
        title: "BrowserDB 官方文档".to_string(),
        visit_count: 1,
    };
    
    db.history().insert(&history_entry)?;
    println!("✅ 历史记录已插入: {}", history_entry.title);
    
    // 查询历史记录
    if let Some(retrieved) = db.history().get(0x123456789abcdef0)? {
        println!("📖 查询到记录: {}", retrieved.title);
    }
    
    // 3. Cookie操作示例
    println!("\n🍪 Cookie操作示例:");
    
    let mut cookie = CookieEntry::new(
        0xabcdef1234567890,
        "session_id".to_string(),
        "abc123xyz789".to_string(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_secs() + 3600,
    );
    
    // 设置Cookie标志
    cookie.set_secure();
    cookie.set_httponly();
    
    db.cookies().insert(&cookie)?;
    println!("✅ Cookie已插入: {} = {}", cookie.name, cookie.value);
    println!("   安全标志: secure={}, httponly={}", cookie.is_secure(), cookie.is_httponly());
    
    // 4. 缓存操作示例
    println!("\n💾 缓存操作示例:");
    
    let cache_entry = CacheEntry {
        url_hash: 0x1111222233334444,
        headers: "Content-Type: text/html; charset=utf-8".to_string(),
        body: b"<!DOCTYPE html><html><head><title>示例页面</title></head><body><h1>BrowserDB 缓存示例</h1></body></html>".to_vec(),
        etag: "W/\"abc123\"".to_string(),
        last_modified: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_millis(),
    };
    
    db.cache().insert(&cache_entry)?;
    println!("✅ 缓存条目已插入 (大小: {} bytes)", cache_entry.body.len());
    
    // 5. 本地存储操作示例
    println!("\n🏠 本地存储操作示例:");
    
    let localstore_entry = LocalStoreEntry {
        origin_hash: 0x5555666677778888,
        key: "user_preferences".to_string(),
        value: r#"{
            "theme": "dark",
            "language": "zh-CN",
            "fontSize": 16,
            "autoplay": false
        }"#.to_string(),
    };
    
    db.localstore().insert(&localstore_entry)?;
    println!("✅ 本地存储已插入: {}", localstore_entry.key);
    
    // 6. 设置操作示例
    println!("\n⚙️  设置操作示例:");
    
    db.settings().set("browser_theme", "dark")?;
    db.settings().set("default_language", "zh-CN")?;
    db.settings().set("cache_size_mb", "100")?;
    
    if let Some(theme) = db.settings().get("browser_theme")? {
        println!("🌙 当前主题: {}", theme);
    }
    
    // 7. 搜索操作示例
    println!("\n🔍 搜索操作示例:");
    
    // 热查询 - 基于访问频率
    let hot_results = db.history().hot_search("BrowserDB", 10)?;
    println!("🔥 热查询结果: {} 条记录", hot_results.len());
    
    // 8. 统计信息
    println!("\n📊 数据库统计信息:");
    let stats = db.stats()?;
    println!("   总条目数: {}", stats.total_entries);
    println!("   历史记录: {}", stats.history_entries);
    println!("   Cookie条目: {}", stats.cookie_entries);
    println!("   缓存条目: {}", stats.cache_entries);
    println!("   内存使用: {} MB", stats.memory_usage_mb);
    println!("   磁盘使用: {} MB", stats.disk_usage_mb);
    
    // 9. 性能测试
    println!("\n⚡ 简单性能测试:");
    
    // 批量插入测试
    let start = std::time::Instant::now();
    for i in 0..100 {
        let entry = HistoryEntry {
            timestamp: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)?
                .as_millis(),
            url_hash: u128::from_le_bytes([i as u8; 16]),
            title: format!("批量插入测试 {}", i),
            visit_count: 1,
        };
        
        db.history().insert(&entry)?;
    }
    
    let elapsed = start.elapsed();
    let throughput = 100.0 / elapsed.as_secs_f64();
    
    println!("   批量插入: 100 条记录");
    println!("   耗时: {:?}", elapsed);
    println!("   吞吐量: {:.0} 条记录/秒", throughput);
    
    // 10. 清理操作
    println!("\n🧹 清理操作:");
    println!("⚠️  即将清理整个数据库 (生产环境中请谨慎使用)");
    
    // 取消注释下面的行来实际执行清理
    // db.wipe()?;
    // println!("✅ 数据库已清理");
    
    println!("\n🎉 示例程序完成!");
    println!("数据库位置: /tmp/example.bdb");
    
    Ok(())
}