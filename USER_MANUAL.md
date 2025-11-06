# 👤 BrowserDB User Manual

Complete guide to using BrowserDB effectively in your applications.

## 📋 Table of Contents

1. [Getting Started](#getting-started)
2. [Database Operations](#database-operations)
3. [Querying Data](#querying-data)
4. [Performance Optimization](#performance-optimization)
5. [Data Management](#data-management)
6. [Error Handling](#error-handling)
7. [Advanced Features](#advanced-features)
8. [Best Practices](#best-practices)
9. [Troubleshooting](#troubleshooting)

---

## 🚀 Getting Started

### Database Creation and Opening

```rust
use browserdb::*;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Create a new database
    let db = BrowserDB::create("my_app.bdb")?;
    
    // Open existing database
    let db = BrowserDB::open("my_app.bdb")?;
    
    // Auto-create if doesn't exist
    let db = BrowserDB::open_or_create("my_app.bdb")?;
    
    Ok(())
}
```

### Database Configuration

```rust
// Default configuration
let db = BrowserDB::open("default.bdb")?;

// Custom configuration
let config = BrowserDBConfig {
    cache_size: 256 * 1024 * 1024,  // 256MB cache
    max_file_size: 64 * 1024 * 1024, // 64MB max file size
    compression: CompressionType::LZ4,
    enable_wal: true,                // Write-ahead logging
    auto_compact: true,             // Automatic compaction
    max_background_jobs: 4,         // Parallel compaction
};

let db = BrowserDB::open_with_config("configured.bdb", config)?;
```

### Database Modes

**Persistent Mode** (Recommended for user data):
```rust
// Disk-backed with intelligent caching
let db = BrowserDB::open_persistent("user_data.bdb")?;
// ✅ Survives application restarts
// ✅ ACID compliance guaranteed
// ✅ Suitable for: user preferences, application state
```

**Ultra Mode** (For high-performance caching):
```rust
// All data in RAM, maximum speed
let db = BrowserDB::open_ultra("cache.bdb")?;
// ✅ Instant access (no disk I/O)
// ✅ Perfect for: temporary data, session cache
// ⚠️ Data lost on restart
```

---

## 🗄️ Database Operations

### Basic CRUD Operations

#### Create/Update (PUT)
```rust
// Simple put
db.put("user:123", "Alice Smith")?;

// Batched puts (more efficient)
let mut batch = db.new_batch();
batch.put("user:123", "Alice Smith");
batch.put("user:456", "Bob Jones");
batch.put("settings:theme", "dark");
batch.commit()?;

// Large data with compression
let large_data = read_large_file("document.pdf");
db.put("document:pdf:123", &large_data)?; // Auto-compressed
```

#### Read (GET)
```rust
// Simple get
if let Some(value) = db.get("user:123")? {
    let user_name = String::from_utf8_lossy(&value);
    println!("User: {}", user_name);
}

// Check existence without retrieving
let exists = db.exists("user:123")?;

// Get with default value
let theme = db.get_or("settings:theme", "light")?;
```

#### Delete (DELETE)
```rust
// Single delete
db.delete("user:123")?;

// Batch delete
let mut batch = db.new_batch();
batch.delete("user:123");
batch.delete("user:456");
batch.delete("temp:data");
batch.commit()?;

// Delete by pattern (prefix)
let deleted_count = db.delete_prefix("temp:")?;
```

### Batch Operations (Recommended)

```rust
// ✅ GOOD: Batch operations for better performance
let mut batch = db.new_batch();

// Add multiple operations
for i in 0..1000 {
    batch.put(&format!("user:{}", i), &format!("User {}", i));
    if i % 100 == 0 {
        batch.commit()?; // Commit every 100 operations
        batch = db.new_batch(); // Start new batch
    }
}

// Final commit
batch.commit()?;

// ❌ BAD: Individual operations (slower)
for i in 0..1000 {
    db.put(&format!("user:{}", i), &format!("User {}", i))?; // Slow!
}
```

### Transaction-like Operations

```rust
// Multi-operation atomic group
let mut transaction = db.begin_transaction()?;

try {
    // All operations succeed or all fail
    transaction.put("user:123", "Alice");
    transaction.put("user:456", "Bob");
    transaction.delete("user:999"); // Maybe doesn't exist
    transaction.commit()?; // Atomic commit
} catch |err| {
    transaction.rollback()?; // Automatic rollback on error
    return Err(err);
}
```

---

## 🔍 Querying Data

### Range Queries

```rust
// Get all users (ordered by key)
let users: Vec<_> = db.range("user:", "user~")?
    .filter_map(|r| r.ok())
    .map(|(key, value)| {
        let user_id = String::from_utf8_lossy(&key).to_string();
        let user_data = String::from_utf8_lossy(&value).to_string();
        (user_id, user_data)
    })
    .collect();

// Numeric range queries
let logs_today = db.range("log:2024-01-15:000000", "log:2024-01-15:235959")?
    .map(|r| String::from_utf8_lossy(&r.1).to_string())
    .collect();

// Case-sensitive ranges
let items_a_to_m = db.range("item:A", "item:N")?.collect::<Result<Vec<_>, _>>()?;
```

### Prefix Queries

```rust
// All users (fast prefix search)
let all_users = db.prefix("user:")?
    .map(|r| String::from_utf8_lossy(&r.1).to_string())
    .collect::<Result<Vec<_>, _>>()?;

// Settings only
let settings: std::collections::HashMap<String, String> = db.prefix("settings:")?
    .filter_map(|r| r.ok())
    .map(|(key, value)| {
        let setting_key = String::from_utf8_lossy(&key).replace("settings:", "");
        let setting_value = String::from_utf8_lossy(&value).to_string();
        (setting_key, setting_value)
    })
    .collect();

// Hierarchical data
let user_123_data = db.prefix("user:123:")?
    .collect::<Result<Vec<_>, _>>()?;
```

### Advanced Queries

```rust
// Paginated results
fn get_users_page(db: &BrowserDB, page: usize, page_size: usize) -> Result<Vec<String>, Box<dyn Error>> {
    let start = format!("user:{:020}", page * page_size);
    let end = format!("user:{:020}", (page + 1) * page_size);
    
    db.range(&start, &end)?
        .map(|r| Ok(String::from_utf8_lossy(&r.1).to_string()))
        .take(page_size)
        .collect()
}

// Conditional updates
fn update_user_if_exists(db: &BrowserDB, user_id: &str, new_data: &str) -> Result<bool, Box<dyn Error>> {
    if db.exists(user_id)? {
        db.put(user_id, new_data)?;
        Ok(true)
    } else {
        Ok(false)
    }
}

// Count operations
fn count_users(db: &BrowserDB) -> Result<usize, Box<dyn Error>> {
    db.prefix("user:")?
        .count()
        .map_err(|e| Box::new(e) as Box<dyn Error>)
}
```

### Iterator Patterns

```rust
// Manual iteration with error handling
let mut iterator = db.prefix("cache:")?;
while let Some(result) = iterator.next()? {
    let (key, value) = result;
    let key_str = String::from_utf8_lossy(&key);
    let value_str = String::from_utf8_lossy(&value);
    
    if key_str.contains("expired") {
        iterator.delete_current()?; // Delete while iterating
    }
}

// Stream processing (memory efficient)
db.prefix("large_data:")?
    .try_for_each(|result| {
        let (key, value) = result?;
        process_large_value(key, value)?;
        Ok(())
    })?;
```

---

## ⚡ Performance Optimization

### Key Design Principles

#### 1. Key Structure
```rust
// ✅ GOOD: Structured keys for efficient queries
"user:123:profile"     // Hierarchical structure
"session:abc123:data"  // Scoped data
"cache:exp:2024-01-15" // Time-based keys
"settings:theme"       // Simple settings
"history:url:hash"     // Content-addressed

// ❌ BAD: Unstructured keys
"user123profile"       // Hard to query
"sessionabc123data"    // No clear separation
"cache20240115"        // No prefix for filtering
"themepreferences"     // Not discoverable
```

#### 2. Data Size Management
```rust
// Keep values reasonably sized (< 1MB)
if large_data.len() > 1024 * 1024 {
    // Store large data in separate files
    let filename = save_to_file(&large_data)?;
    db.put("document:123:file", filename.as_bytes())?;
    db.put("document:123:metadata", metadata_json)?;
} else {
    db.put("document:123", &large_data)?;
}

// Use compression for text data
let text_data = serde_json::to_string(&user_data)?;
db.put("user:123", text_data.as_bytes())?; // Auto-compressed
```

#### 3. Query Optimization
```rust
// ✅ Efficient: Use prefix queries
let users = db.prefix("user:")?
    .filter(|r| r.1.len() < 100)  // Filter small values
    .take(50)                     // Limit results
    .collect::<Result<Vec<_>, _>>()?;

// ❌ Inefficient: Load everything then filter
let all_users = db.prefix("user:")?.collect::<Result<Vec<_>, _>>()?;
let filtered_users: Vec<_> = all_users
    .into_iter()
    .filter(|r| r.1.len() < 100)
    .take(50)
    .collect();
```

### Memory Management

```rust
// Monitor memory usage
let memory_stats = db.get_memory_statistics()?;
println!("Cache usage: {}MB", memory_stats.cache_usage / 1024 / 1024);
println!("Heap usage: {}MB", memory_stats.heap_usage / 1024 / 1024);
println!("Cache hit rate: {:.1}%", memory_stats.cache_hit_rate * 100.0);

// Optimize cache size based on workload
if memory_stats.cache_hit_rate < 0.90 {
    // Increase cache for better performance
    db.resize_cache(memory_stats.cache_usage * 2)?;
}
```

### Compaction Management

```rust
// Manual compaction for maintenance
db.compact()?;

// Compact with specific strategy
let strategy = CompactionConfig {
    target_file_size: 32 * 1024 * 1024, // 32MB files
    max_level_size: 1024 * 1024 * 1024, // 1GB per level
    enable_tiered: true,
};
db.compact_with_config(strategy)?;

// Get compaction statistics
let stats = db.get_compaction_stats()?;
println!("Compacted {} files, saved {}MB", stats.files_compacted, stats.bytes_saved / 1024 / 1024);
```

---

## 📊 Data Management

### Backup and Restore

```rust
// Create backup
fn backup_database(db: &BrowserDB, backup_path: &str) -> Result<(), Box<dyn Error>> {
    // Atomic backup operation
    let backup = db.create_backup(backup_path)?;
    backup.wait_for_completion()?;
    Ok(())
}

// Restore from backup
fn restore_database(db_path: &str, backup_path: &str) -> Result<BrowserDB, Box<dyn Error>> {
    let backup = Backup::open(backup_path)?;
    backup.restore_to(db_path)?;
    BrowserDB::open(db_path)
}

// Incremental backup
let last_backup = db.get_last_backup_time()?;
if last_backup.elapsed()? > std::time::Duration::from_hours(24) {
    db.create_incremental_backup("backup_20240115.bak")?;
}
```

### Data Migration

```rust
// Schema migration example
fn migrate_user_schema(db: &BrowserDB) -> Result<(), Box<dyn Error>> {
    let mut transaction = db.begin_transaction()?;
    
    // Read old format
    let old_users = db.prefix("users:old:")?
        .collect::<Result<Vec<_>, _>>()?;
    
    // Transform to new format
    for (key, value) in old_users {
        let old_user: OldUser = serde_json::from_slice(&value)?;
        let new_user = NewUser::from(old_user);
        
        let new_key = key.replace("users:old:", "users:");
        transaction.put(&new_key, serde_json::to_string(&new_user)?.as_bytes())?;
        transaction.delete(&key)?; // Remove old format
    }
    
    transaction.commit()?;
    Ok(())
}
```

### Data Import/Export

```rust
// Export to JSON
fn export_to_json(db: &BrowserDB, output_path: &str) -> Result<(), Box<dyn Error>> {
    let mut writer = std::fs::File::create(output_path)?;
    writer.write_all(b"[\n")?;
    
    let mut first = true;
    for result in db.prefix("")? {
        let (key, value) = result?;
        if !first {
            writer.write_all(b",\n")?;
        }
        first = false;
        
        let record = serde_json::json!({
            "key": String::from_utf8_lossy(&key),
            "value": String::from_utf8_lossy(&value)
        });
        writer.write_all(serde_json::to_string_pretty(&record)?.as_bytes())?;
    }
    
    writer.write_all(b"\n]")?;
    Ok(())
}

// Import from JSON
fn import_from_json(db: &BrowserDB, input_path: &str) -> Result<(), Box<dyn Error>> {
    let content = std::fs::read_to_string(input_path)?;
    let records: Vec<serde_json::Value> = serde_json::from_str(&content)?;
    
    let mut batch = db.new_batch();
    for record in records {
        let key = record["key"].as_str().unwrap();
        let value = record["value"].as_str().unwrap();
        batch.put(key.as_bytes(), value.as_bytes())?;
    }
    batch.commit()?;
    Ok(())
}
```

---

## 🚨 Error Handling

### Common Error Types

```rust
match db.get("user:999") {
    Ok(Some(value)) => println!("Found: {:?}", value),
    Ok(None) => println!("User not found"),
    Err(DBError::NotFound) => println!("Key does not exist"),
    Err(DBError::CorruptionDetected) => {
        println!("Data corruption detected!");
        // Attempt recovery
        if let Err(recovery_error) = db.recover() {
            eprintln!("Recovery failed: {}", recovery_error);
        }
    },
    Err(DBError::IOError(msg)) => println!("I/O error: {}", msg),
    Err(DBError::InvalidArgument(msg)) => println!("Invalid argument: {}", msg),
    Err(DBError::OutOfMemory) => {
        println!("Out of memory - consider reducing cache size");
        db.resize_cache(db.get_cache_size() / 2)?;
    },
}
```

### Robust Error Handling Patterns

```rust
fn robust_operation(db: &BrowserDB, key: &str, value: &str) -> Result<(), Box<dyn Error>> {
    // Retry logic for transient errors
    for attempt in 0..3 {
        match db.put(key, value) {
            Ok(()) => return Ok(()),
            Err(DBError::IOError(_)) if attempt < 2 => {
                std::thread::sleep(std::time::Duration::from_millis(100 * (attempt + 1)));
                continue;
            },
            Err(e) => return Err(Box::new(e)),
        }
    }
    unreachable!()
}

// Circuit breaker pattern
struct DatabaseCircuit {
    failure_count: usize,
    last_failure_time: Option<std::time::Instant>,
    threshold: usize,
}

impl DatabaseCircuit {
    fn new() -> Self {
        Self {
            failure_count: 0,
            last_failure_time: None,
            threshold: 5,
        }
    }
    
    fn execute<F, T>(&mut self, operation: F) -> Result<T, Box<dyn Error>>
    where
        F: FnOnce() -> Result<T, DBError>,
    {
        // Check if circuit is open
        if let Some(last_failure) = self.last_failure_time {
            if self.failure_count >= self.threshold && 
               last_failure.elapsed()? < std::time::Duration::from_secs(60) {
                return Err("Circuit breaker is open".into());
            }
        }
        
        match operation() {
            Ok(result) => {
                self.failure_count = 0;
                Ok(result)
            },
            Err(error) => {
                self.failure_count += 1;
                self.last_failure_time = Some(std::time::Instant::now());
                Err(Box::new(error))
            }
        }
    }
}
```

---

## 🔧 Advanced Features

### Custom Compression

```rust
// Custom compression configuration
let config = BrowserDBConfig {
    cache_size: 512 * 1024 * 1024,
    compression: CompressionType::LZ4, // Fast compression
    compression_threshold: 1024,       // Compress if > 1KB
    ..Default::default()
};

// Mixed compression strategies
let db = BrowserDB::open_with_config("mixed_compression.bdb", config)?;

// Use different compression for different data types
db.put("user:123:avatar", &large_image_data)?; // Uses LZ4 (fast)
db.put("user:123:profile", &profile_json)?;    // Uses LZ4 (moderate size)
db.put("system:logs", &log_data)?;             // Uses LZ4 (streaming)
```

### Performance Monitoring

```rust
// Detailed performance statistics
let stats = db.get_performance_stats()?;
println!("=== Performance Statistics ===");
println!("Reads: {} operations/sec", stats.reads_per_second);
println!("Writes: {} operations/sec", stats.writes_per_second);
println!("Cache hit rate: {:.2}%", stats.cache_hit_rate * 100.0);
println!("Average query time: {:.2}ms", stats.average_query_time_ms);
println!("Memory usage: {}MB", stats.memory_usage_mb);
println!("Disk usage: {}MB", stats.disk_usage_mb);

// Monitor specific operations
let timer = db.start_timer();
let result = db.get("user:123")?;
let duration = timer.elapsed();
if duration.as_millis() > 100 {
    println!("Slow query detected: {}ms", duration.as_millis());
}
```

### Multi-Database Management

```rust
// Database registry for multiple databases
struct DatabaseRegistry {
    user_db: BrowserDB,
    cache_db: BrowserDB,
    analytics_db: BrowserDB,
}

impl DatabaseRegistry {
    fn new() -> Result<Self, Box<dyn Error>> {
        Ok(Self {
            user_db: BrowserDB::open_persistent("users.bdb")?,
            cache_db: BrowserDB::open_ultra("cache.bdb")?,
            analytics_db: BrowserDB::open_persistent("analytics.bdb")?,
        })
    }
    
    fn get_user_data(&self, user_id: &str) -> Result<Option<String>, Box<dyn Error>> {
        // Try cache first
        if let Some(data) = self.cache_db.get(&format!("user:{}", user_id))? {
            return Ok(Some(String::from_utf8_lossy(&data).to_string()));
        }
        
        // Fall back to persistent storage
        if let Some(data) = self.user_db.get(&format!("user:{}", user_id))? {
            // Update cache
            self.cache_db.put(&format!("user:{}", user_id), &data)?;
            return Ok(Some(String::from_utf8_lossy(&data).to_string()));
        }
        
        Ok(None)
    }
}
```

---

## ✅ Best Practices

### 1. Key Naming Conventions

```rust
// Use descriptive, hierarchical keys
"user:123:profile"           // User data
"user:123:settings"          // User settings
"session:abc123:data"        // Session data
"cache:url:hash"             // URL cache
"analytics:event:type"       // Analytics events

// Include timestamps for time-series data
"log:2024-01-15:user:123"    // Timestamped logs
"metric:cpu:2024-01-15"      // Metrics

// Use consistent separators
"user:123"     ✅ Good
"user/123"     ❌ Inconsistent
"user123"      ❌ No separation
```

### 2. Data Size Guidelines

```rust
// Keep values under 1MB for optimal performance
let large_data = std::fs::read("document.pdf")?;
if large_data.len() > 1024 * 1024 {
    // Store large files separately
    let file_id = save_large_file(&large_data)?;
    db.put("document:123:file_id", file_id.as_bytes())?;
    db.put("document:123:metadata", &create_metadata(&large_data)?)?;
} else {
    db.put("document:123", &large_data)?;
}

// Use appropriate compression
let json_data = serde_json::to_string(&user_data)?;
db.put("user:123", json_data.as_bytes())?; // Text compresses well
db.put("user:123:avatar", &image_bytes)?;   // Binary data, may not compress well
```

### 3. Query Optimization

```rust
// Use specific prefixes for faster queries
let all_users = db.prefix("user:")?;              // Fast: uses prefix index
let range_users = db.range("user:", "user~")?;    // Slower: needs range scan

// Limit result sets for large datasets
let recent_logs = db.prefix("log:2024-01-15")?
    .take(1000)  // Limit to prevent memory issues
    .collect::<Result<Vec<_>, _>>()?;

// Use batch operations for bulk changes
let mut batch = db.new_batch();
for i in 0..10000 {
    batch.put(&format!("batch:{}", i), &format!("data:{}", i));
    if i % 1000 == 0 {
        batch.commit()?; // Commit every 1000 operations
        batch = db.new_batch();
    }
}
```

### 4. Error Recovery

```rust
// Always handle potential corruption
match db.get("critical:data") {
    Ok(Some(data)) => process_data(data),
    Ok(None) => log::warn!("Data not found"),
    Err(DBError::CorruptionDetected) => {
        log::error!("Data corruption detected!");
        // Attempt automatic recovery
        if let Err(e) = db.recover() {
            log::error!("Recovery failed: {}", e);
            // Fall back to backup
            restore_from_backup("backup.bak")?;
        }
    },
    Err(e) => {
        log::error!("Database error: {}", e);
        return Err(e.into());
    }
}
```

### 5. Memory Management

```rust
// Monitor and adjust cache size
let stats = db.get_performance_stats()?;
if stats.cache_hit_rate < 0.90 {
    // Cache hit rate too low, increase cache
    let new_cache_size = std::cmp::min(
        stats.current_cache_size * 2,
        1024 * 1024 * 1024 // Max 1GB
    );
    db.resize_cache(new_cache_size)?;
} else if stats.memory_usage_mb > 1000 {
    // Memory usage too high, decrease cache
    db.resize_cache(stats.current_cache_size / 2)?;
}

// Clean up old data periodically
fn cleanup_old_data(db: &BrowserDB, days_to_keep: i32) -> Result<usize, Box<dyn Error>> {
    let cutoff = std::time::SystemTime::now() - std::time::Duration::from_secs(86400 * days_to_keep as u64);
    let mut deleted_count = 0;
    
    for result in db.prefix("temp:")? {
        let (key, _) = result?;
        // Parse timestamp from key and compare with cutoff
        if is_expired(&key, cutoff) {
            db.delete(&key)?;
            deleted_count += 1;
        }
    }
    
    Ok(deleted_count)
}
```

---

## 🔍 Troubleshooting

### Performance Issues

**Slow Queries:**
```rust
// Check cache hit rate
let stats = db.get_performance_stats()?;
if stats.cache_hit_rate < 0.90 {
    println!("Low cache hit rate detected!");
    db.resize_cache(stats.current_cache_size * 2)?;
}

// Check for large values
let mut large_value_count = 0;
for result in db.prefix("")? {
    let (_, value) = result?;
    if value.len() > 1024 * 1024 {
        large_value_count += 1;
    }
}
if large_value_count > 0 {
    println!("Found {} large values (>1MB)", large_value_count);
}
```

**High Memory Usage:**
```rust
// Monitor memory usage
let memory_stats = db.get_memory_statistics()?;
println!("Cache: {}MB", memory_stats.cache_usage / 1024 / 1024);
println!("Heap: {}MB", memory_stats.heap_usage / 1024 / 1024);

// Reduce cache if needed
if memory_stats.cache_usage > 500 * 1024 * 1024 {
    db.resize_cache(256 * 1024 * 1024)?;
}

// Force garbage collection
db.force_gc()?;
```

### Data Corruption

```rust
// Detect corruption early
fn verify_database_integrity(db: &BrowserDB) -> Result<bool, Box<dyn Error>> {
    match db.validate() {
        Ok(ValidationResult::Healthy) => {
            println!("Database integrity verified");
            Ok(true)
        },
        Ok(ValidationResult::Warnings(warnings)) => {
            println!("Database has warnings: {:?}", warnings);
            // Proceed with caution
            Ok(true)
        },
        Err(DBError::CorruptionDetected) => {
            println!("Database corruption detected!");
            // Attempt repair
            if let Err(repair_error) = db.repair() {
                eprintln!("Repair failed: {}", repair_error);
                Ok(false)
            } else {
                println!("Database repaired successfully");
                Ok(true)
            }
        },
        Err(e) => {
            eprintln!("Validation error: {}", e);
            Ok(false)
        }
    }
}
```

### Common Error Solutions

| Error | Cause | Solution |
|-------|-------|----------|
| `OutOfMemory` | Cache too large | `db.resize_cache(smaller_size)` |
| `CorruptionDetected` | File system issues | `db.repair()` or restore backup |
| `IOError` | Disk full or permissions | Check disk space and permissions |
| `NotFound` | Normal operation | Check key exists with `db.exists()` |
| `InvalidArgument` | Bad parameters | Validate inputs before calling |

### Recovery Procedures

```rust
// Complete recovery procedure
fn emergency_recovery(db_path: &str) -> Result<(), Box<dyn Error>> {
    println!("Starting emergency recovery...");
    
    // 1. Create backup of current state
    let backup_path = format!("{}.corrupted_backup", db_path);
    std::fs::copy(db_path, &backup_path)?;
    println!("Created backup: {}", backup_path);
    
    // 2. Attempt automatic repair
    let db = BrowserDB::open(db_path)?;
    match db.repair() {
        Ok(()) => {
            println!("Automatic repair successful");
            return Ok(());
        },
        Err(e) => {
            println!("Automatic repair failed: {}", e);
        }
    }
    
    // 3. Manual recovery from backup
    let backup_files: Vec<_> = std::fs::read_dir("backups")?
        .filter_map(|entry| entry.ok())
        .filter(|entry| entry.file_name().to_str()?.ends_with(".bak"))
        .collect();
    
    if let Some(backup) = backup_files.first() {
        println!("Restoring from backup: {:?}", backup.file_name());
        let backup_path = backup.path();
        std::fs::copy(&backup_path, db_path)?;
        println!("Recovery completed");
    } else {
        return Err("No backup files found".into());
    }
    
    Ok(())
}
```

---

<div align="center">

**[⬅️ Back to Quick Start](QUICK_START.md)** | **[🏠 README](README.md)** | **[🛠️ Developer Guide](DEVELOPER_GUIDE.md)**

Master these patterns to build robust, high-performance applications!

</div>