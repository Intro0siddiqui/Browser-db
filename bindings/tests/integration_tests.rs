//! BrowserDB Rust integration tests

use browserdb::*;
use tempfile::tempdir;
use std::time::Duration;

#[tokio::test]
async fn test_database_creation_and_basic_operations() {
    let temp_dir = tempdir().unwrap();
    let db_path = temp_dir.path().join("test.bdb");
    
    // 创建数据库
    let db = BrowserDB::open(db_path.to_str().unwrap()).expect("Failed to create database");
    
    // 测试基本统计信息
    let stats = db.stats().expect("Failed to get stats");
    println!("Initial stats: {:?}", stats);
    
    // 测试模式切换
    db.set_mode(DatabaseMode::Ultra).expect("Failed to set mode");
    
    // 测试基本插入
    let history_entry = HistoryEntry {
        timestamp: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis(),
        url_hash: 0x123456789abcdef0,
        title: "Test Page".to_string(),
        visit_count: 1,
    };
    
    db.history().insert(&history_entry).expect("Failed to insert history");
    
    // 验证插入
    let retrieved = db.history().get(0x123456789abcdef0).expect("Failed to get history");
    assert!(retrieved.is_some());
    
    if let Some(entry) = retrieved {
        assert_eq!(entry.title, "Test Page");
        assert_eq!(entry.visit_count, 1);
    }
}

#[tokio::test]
async fn test_cookie_operations() {
    let temp_dir = tempdir().unwrap();
    let db_path = temp_dir.path().join("cookie_test.bdb");
    
    let db = BrowserDB::open(db_path.to_str().unwrap()).expect("Failed to create database");
    
    let cookie = CookieEntry {
        domain_hash: 0xabcdef1234567890,
        name: "session_id".to_string(),
        value: "abc123xyz789".to_string(),
        expiry: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs()
            + 3600, // 1小时后过期
        flags: cookie_flags::SECURE,
    };
    
    // 插入Cookie
    db.cookies().insert(&cookie).expect("Failed to insert cookie");
    
    // 验证Cookie标志
    assert!(cookie.is_secure());
    assert!(!cookie.is_httponly());
}

#[tokio::test]
async fn test_cache_operations() {
    let temp_dir = tempdir().unwrap();
    let db_path = temp_dir.path().join("cache_test.bdb");
    
    let db = BrowserDB::open(db_path.to_str().unwrap()).expect("Failed to create database");
    
    let cache_entry = CacheEntry {
        url_hash: 0x1111222233334444,
        headers: "Content-Type: text/html".to_string(),
        body: b"<html><body>Test Content</body></html>".to_vec(),
        etag: "W/\"abc123\"".to_string(),
        last_modified: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis(),
    };
    
    // 插入缓存条目
    db.cache().insert(&cache_entry).expect("Failed to insert cache");
    
    // 验证插入
    let retrieved = db.cache().get(0x1111222233334444).expect("Failed to get cache");
    assert!(retrieved.is_some());
    
    if let Some(entry) = retrieved {
        assert_eq!(entry.headers, "Content-Type: text/html");
        assert_eq!(entry.body, b"<html><body>Test Content</body></html>");
    }
}

#[tokio::test]
async fn test_localstorage_operations() {
    let temp_dir = tempdir().unwrap();
    let db_path = temp_dir.path().join("localstore_test.bdb");
    
    let db = BrowserDB::open(db_path.to_str().unwrap()).expect("Failed to create database");
    
    let localstore_entry = LocalStoreEntry {
        origin_hash: 0x5555666677778888,
        key: "user_preference".to_string(),
        value: r#"{"theme": "dark", "language": "en"}"#.to_string(),
    };
    
    // 插入本地存储
    db.localstore().insert(&localstore_entry).expect("Failed to insert localstore");
    
    // 按源获取数据
    let entries = db.localstore()
        .get_by_origin(0x5555666677778888)
        .expect("Failed to get localstore by origin");
    
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].key, "user_preference");
}

#[tokio::test]
async fn test_settings_operations() {
    let temp_dir = tempdir().unwrap();
    let db_path = temp_dir.path().join("settings_test.bdb");
    
    let db = BrowserDB::open(db_path.to_str().unwrap()).expect("Failed to create database");
    
    // 设置配置项
    db.settings().set("theme", "dark").expect("Failed to set theme");
    db.settings().set("language", "en").expect("Failed to set language");
    
    // 获取配置项
    let theme = db.settings().get("theme").expect("Failed to get theme");
    let language = db.settings().get("language").expect("Failed to get language");
    
    assert_eq!(theme, Some("dark".to_string()));
    assert_eq!(language, Some("en".to_string()));
}

#[tokio::test]
async fn test_privacy_wipe_operations() {
    let temp_dir = tempdir().unwrap();
    let db_path = temp_dir.path().join("privacy_test.bdb");
    
    let db = BrowserDB::open(db_path.to_str().unwrap()).expect("Failed to create database");
    
    // 插入测试数据
    let history_entry = HistoryEntry {
        timestamp: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis(),
        url_hash: 0x9999aaaabbbbcccc,
        title: "evil.com tracking page".to_string(),
        visit_count: 5,
    };
    
    db.history().insert(&history_entry).expect("Failed to insert history");
    
    // 模拟隐私清理
    let result = db.history()
        .wipe_domain("evil.com")
        .expect("Failed to wipe domain");
    
    println!("Privacy wipe result: removed {} entries", result);
}

#[tokio::test]
async fn test_performance_operations() {
    let temp_dir = tempdir().unwrap();
    let db_path = temp_dir.path().join("perf_test.bdb");
    
    let db = BrowserDB::open(db_path.to_str().unwrap()).expect("Failed to create database");
    
    // 批量插入历史记录进行性能测试
    let start_time = std::time::Instant::now();
    let num_entries = 1000;
    
    for i in 0..num_entries {
        let entry = HistoryEntry {
            timestamp: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_millis(),
            url_hash: u128::from_le_bytes([i as u8; 16]),
            title: format!("Performance Test Entry {}", i),
            visit_count: 1,
        };
        
        db.history().insert(&entry).expect("Failed to insert entry");
    }
    
    let elapsed = start_time.elapsed();
    let throughput = num_entries as f64 / elapsed.as_secs_f64();
    
    println!("Performance test:");
    println!("  Entries: {}", num_entries);
    println!("  Time: {:?}", elapsed);
    println!("  Throughput: {:.0} entries/sec", throughput);
    
    // 验证性能目标
    assert!(throughput > 5000.0, "Write throughput too low: {:.0} entries/sec", throughput);
}

#[tokio::test]
async fn test_database_wipe() {
    let temp_dir = tempdir().unwrap();
    let db_path = temp_dir.path().join("wipe_test.bdb");
    
    let db = BrowserDB::open(db_path.to_str().unwrap()).expect("Failed to create database");
    
    // 插入一些数据
    let entry = HistoryEntry {
        timestamp: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis(),
        url_hash: 0xdeadbeefcafebabe,
        title: "Test Entry".to_string(),
        visit_count: 1,
    };
    
    db.history().insert(&entry).expect("Failed to insert entry");
    
    // 验证数据存在
    let retrieved = db.history().get(0xdeadbeefcafebabe).expect("Failed to get entry");
    assert!(retrieved.is_some());
    
    // 清理数据库
    db.wipe().expect("Failed to wipe database");
    
    // 验证数据已被清理
    let stats = db.stats().expect("Failed to get stats after wipe");
    assert_eq!(stats.total_entries, 0);
}

#[test]
fn test_database_mode_switching() {
    let temp_dir = tempdir().unwrap();
    let db_path = temp_dir.path().join("mode_test.bdb");
    
    let db = BrowserDB::open(db_path.to_str().unwrap()).expect("Failed to create database");
    
    // 测试Persistent模式
    db.set_mode(DatabaseMode::Persistent).expect("Failed to set persistent mode");
    
    // 测试Ultra模式
    db.set_mode(DatabaseMode::Ultra).expect("Failed to set ultra mode");
}

#[test]
fn test_error_handling() {
    let temp_dir = tempdir().unwrap();
    let db_path = temp_dir.path().join("error_test.bdb");
    
    let db = BrowserDB::open(db_path.to_str().unwrap()).expect("Failed to create database");
    
    // 测试不存在的URL哈希查询
    let result = db.history().get(0x9999999999999999);
    assert!(result.is_ok()); // 应该返回Ok(None)而不是错误
}