# 🔧 BrowserDB API Reference

Complete API documentation for all BrowserDB functions and types.

## 📋 Table of Contents

1. [Core Database API](#core-database-api)
2. [Query and Iteration](#query-and-iteration)
3. [Configuration](#configuration)
4. [Error Handling](#error-handling)
5. [Performance Monitoring](#performance-monitoring)
6. [Backup and Recovery](#backup-and-recovery)
7. [Rust FFI Bindings](#rust-ffi-bindings)

---

## 🗄️ Core Database API

### BrowserDB

Main database orchestrator class.

#### Constructors

```rust
pub fn open(path: &str) -> Result<BrowserDB, DBError>
```

Opens an existing database or creates a new one.

**Parameters:**
- `path`: Path to database file

**Returns:**
- `Ok(BrowserDB)`: Database instance
- `Err(DBError)`: Error code

**Examples:**
```rust
let db = BrowserDB::open("myapp.bdb")?;

let db = BrowserDB::open("path/to/database.bdb")?;
```

---

```rust
pub fn open_or_create(path: &str) -> Result<BrowserDB, DBError>
```

Opens existing database or creates new one.

**Examples:**
```rust
let db = BrowserDB::open_or_create("new_app.bdb")?;
```

---

```rust
pub fn create(path: &str) -> Result<BrowserDB, DBError>
```

Creates a new database (fails if exists).

**Examples:**
```rust
let db = BrowserDB::create("fresh_database.bdb")?;
```

#### Mode-Specific Constructors

```rust
pub fn open_persistent(path: &str) -> Result<BrowserDB, DBError>
```

Opens database in persistent mode (disk-backed).

**Examples:**
```rust
let db = BrowserDB::open_persistent("user_data.bdb")?;
```

---

```rust
pub fn open_ultra(path: &str) -> Result<BrowserDB, DBError>
```

Opens database in ultra mode (RAM-only).

**Examples:**
```rust
let db = BrowserDB::open_ultra("cache.bdb")?;
```

#### Database Operations

##### Basic CRUD

```rust
pub fn put(&self, key: &[u8], value: &[u8]) -> Result<(), DBError>
```

Stores a key-value pair in the database.

**Parameters:**
- `key`: Key bytes (can be any data)
- `value`: Value bytes (can be any data)

**Returns:**
- `Ok(())`: Success
- `Err(DBError)`: Error code

**Examples:**
```rust
// Store string data
db.put(b"user:123", b"Alice Smith")?;

// Store structured data
let user_data = serde_json::to_vec(&user)?;
db.put(b"user:123", &user_data)?;

// Store binary data
db.put(b"avatar:123", &image_bytes)?;
```

**Error Cases:**
- `DBError::ValueTooLarge`: Value exceeds maximum size
- `DBError::OutOfMemory`: Insufficient memory
- `DBError::IOError`: File system error

---

```rust
pub fn get(&self, key: &[u8]) -> Result<Option<Vec<u8>>, DBError>
```

Retrieves a value by key.

**Parameters:**
- `key`: Key to lookup

**Returns:**
- `Ok(Some(Vec<u8>))`: Value found
- `Ok(None)`: Key not found
- `Err(DBError)`: Error code

**Examples:**
```rust
match db.get(b"user:123")? {
    Some(value) => {
        let user_name = String::from_utf8_lossy(&value);
        println!("User: {}", user_name);
    },
    None => println!("User not found"),
}

// Direct unwrap with default
let value = db.get(b"settings:theme")?.unwrap_or_else(|| b"light".to_vec());
```

---

```rust
pub fn delete(&self, key: &[u8]) -> Result<(), DBError>
```

Deletes a key-value pair.

**Parameters:**
- `key`: Key to delete

**Returns:**
- `Ok(())`: Success
- `Err(DBError)`: Error code

**Examples:**
```rust
// Delete single record
db.delete(b"user:123")?;

// Delete with existence check
if db.exists(b"temp:data")? {
    db.delete(b"temp:data")?;
}
```

---

```rust
pub fn exists(&self, key: &[u8]) -> Result<bool, DBError>
```

Checks if a key exists without retrieving the value.

**Parameters:**
- `key`: Key to check

**Returns:**
- `Ok(true)`: Key exists
- `Ok(false)`: Key not found
- `Err(DBError)`: Error code

**Examples:**
```rust
if db.exists(b"user:123")? {
    // User exists, proceed with update
    db.put(b"user:123", new_data)?;
} else {
    // User doesn't exist, create new
    db.put(b"user:123", initial_data)?;
}
```

##### Batch Operations

```rust
pub fn new_batch(&self) -> Batch
```

Creates a new batch for atomic multi-operation execution.

**Examples:**
```rust
let mut batch = db.new_batch();

// Add multiple operations
batch.put(b"user:123", b"Alice");
batch.put(b"user:124", b"Bob");
batch.put(b"user:125", b"Charlie");

// Commit all at once
batch.commit()?;
```

---

#### Batch

Interface for batch operations.

```rust
pub struct Batch {
    operations: Vec<BatchOperation>,
}

pub fn put(&mut self, key: &[u8], value: &[u8]) -> &mut Batch
```

Adds a put operation to the batch.

**Examples:**
```rust
let mut batch = db.new_batch();
batch.put(b"key1", b"value1").put(b"key2", b"value2");
batch.commit()?;
```

---

```rust
pub fn delete(&mut self, key: &[u8]) -> &mut Batch
```

Adds a delete operation to the batch.

**Examples:**
```rust
let mut batch = db.new_batch();
batch.delete(b"old_key").delete(b"another_old_key");
batch.commit()?;
```

---

```rust
pub fn commit(&mut self) -> Result<(), DBError>
```

Executes all operations in the batch atomically.

**Returns:**
- `Ok(())`: All operations succeeded
- `Err(DBError)`: Batch failed, no changes applied

**Examples:**
```rust
let mut batch = db.new_batch();
for i in 0..1000 {
    batch.put(&format!("user:{}", i).into_bytes(), 
              &format!("User {}", i).into_bytes());
}

// Commit every 100 operations for better performance
batch.commit()?;
```

---

## 🔍 Query and Iteration

### Range Queries

```rust
pub fn range(&self, start: &[u8], end: &[u8]) -> Result<RangeIterator, DBError>
```

Iterates over key-value pairs in a key range.

**Parameters:**
- `start`: Starting key (inclusive)
- `end`: Ending key (exclusive)

**Returns:**
- `Ok(RangeIterator)`: Iterator over matching pairs
- `Err(DBError)`: Error code

**Examples:**
```rust
// Get all users
let users: Vec<_> = db.range(b"user:", b"user~")?
    .filter_map(|r| r.ok())
    .collect();

// Numeric range (using zero-padded keys)
let items_100_to_199 = db.range(b"item:0000000100", b"item:0000000199")?
    .map(|r| String::from_utf8_lossy(&r.1).to_string())
    .collect();
```

---

### Prefix Queries

```rust
pub fn prefix(&self, prefix: &[u8]) -> Result<PrefixIterator, DBError>
```

Iterates over all keys with a given prefix.

**Parameters:**
- `prefix`: Key prefix to match

**Returns:**
- `Ok(PrefixIterator)`: Iterator over matching pairs
- `Err(DBError)`: Error code

**Examples:**
```rust
// All users
let users = db.prefix(b"user:")?
    .map(|r| String::from_utf8_lossy(&r.1).to_string())
    .collect::<Result<Vec<_>, _>>()?;

// All settings
let settings: std::collections::HashMap<String, String> = db.prefix(b"settings:")?
    .filter_map(|r| r.ok())
    .map(|(key, value)| {
        let key_str = String::from_utf8_lossy(&key).replace("settings:", "");
        let value_str = String::from_utf8_lossy(&value).to_string();
        (key_str, value_str)
    })
    .collect();

// Hierarchical data (user 123's data)
let user_123_data = db.prefix(b"user:123:")?
    .collect::<Result<Vec<_>, _>>()?;
```

---

### Iterators

#### RangeIterator

```rust
pub struct RangeIterator<'a> {
    db: &'a BrowserDB,
    // Internal iterator state
}

impl<'a> Iterator for RangeIterator<'a> {
    type Item = Result<(Vec<u8>, Vec<u8>), DBError>;
    
    fn next(&mut self) -> Option<Self::Item> {
        // Iterator implementation
    }
}
```

**Examples:**
```rust
let mut iterator = db.range(b"user:", b"user~")?;

while let Some(result) = iterator.next() {
    match result {
        Ok((key, value)) => {
            let user_name = String::from_utf8_lossy(&value);
            println!("Found user: {}", user_name);
        },
        Err(error) => {
            eprintln!("Iterator error: {}", error);
            break;
        }
    }
}
```

#### PrefixIterator

```rust
pub struct PrefixIterator<'a> {
    db: &'a BrowserDB,
    // Internal iterator state
}
```

**Methods:**
```rust
pub fn count(&mut self) -> Result<usize, DBError>
```

Counts matching entries without collecting them (memory efficient).

**Examples:**
```rust
let user_count = db.prefix(b"user:")?.count()?;
println!("Total users: {}", user_count);
```

---

### Iterator Utilities

#### Stream Processing

```rust
pub fn try_for_each<F, E>(&mut self, f: F) -> Result<(), E>
where
    F: FnMut(Result<(Vec<u8>, Vec<u8>), DBError>) -> Result<(), E>,
    E: From<DBError> + From<serde_json::Error>,
```

Processes iterator items with early termination on error.

**Examples:**
```rust
// Process large datasets efficiently
db.prefix(b"cache:")?.try_for_each(|result| {
    let (key, value) = result?;
    let key_str = String::from_utf8_lossy(&key);
    
    // Remove expired cache entries
    if key_str.contains("expired") {
        let db_ref = get_db_reference(); // Access database for deletion
        db_ref.delete(&key)?;
    }
    
    Ok(())
})?;
```

---

#### Pagination

```rust
pub fn take(&mut self, limit: usize) -> Take<Self>
pub fn skip(&mut self, offset: usize) -> Skip<Self>
```

Iterator adaptors for pagination.

**Examples:**
```rust
// Get page 2 (20 items per page)
let page = 2;
let page_size = 20;
let start_offset = (page - 1) * page_size;

let users: Vec<_> = db.prefix(b"user:")?
    .skip(start_offset)
    .take(page_size)
    .map(|r| String::from_utf8_lossy(&r.1).to_string())
    .collect::<Result<Vec<_>, _>>()?;
```

---

## ⚙️ Configuration

### DatabaseConfig

```rust
pub struct DatabaseConfig {
    pub cache_size: usize,
    pub max_file_size: usize,
    pub compression: CompressionType,
    pub enable_wal: bool,
    pub auto_compact: bool,
    pub max_background_jobs: usize,
}
```

**Fields:**
- `cache_size`: Cache memory limit in bytes (default: 64MB)
- `max_file_size`: Maximum SSTable file size (default: 64MB)
- `compression`: Compression algorithm to use
- `enable_wal`: Enable write-ahead logging (default: true)
- `auto_compact`: Enable automatic compaction (default: true)
- `max_background_jobs`: Maximum parallel background operations (default: 4)

**Examples:**
```rust
let config = DatabaseConfig {
    cache_size: 256 * 1024 * 1024,  // 256MB cache
    max_file_size: 128 * 1024 * 1024, // 128MB files
    compression: CompressionType::LZ4,
    enable_wal: true,
    auto_compact: true,
    max_background_jobs: 8,
};

let db = BrowserDB::open_with_config("configured.bdb", config)?;
```

---

### CompressionType

```rust
pub enum CompressionType {
    None,
    LZ4,
    Zlib,
    Zstandard,
}
```

**Characteristics:**

| Algorithm | Speed | Compression Ratio | Best For |
|-----------|-------|-------------------|----------|
| `None` | Fastest | None | Small values, maximum speed |
| `LZ4` | Very Fast | Low-Moderate | General purpose |
| `Zlib` | Fast | Moderate | Text data |
| `Zstandard` | Medium | High | Large text data |

**Examples:**
```rust
// Use LZ4 for general purpose
let config = DatabaseConfig {
    compression: CompressionType::LZ4,
    ..Default::default()
};

// Use Zstandard for maximum compression
let config = DatabaseConfig {
    compression: CompressionType::Zstandard,
    cache_size: 512 * 1024 * 1024, // More cache for Zstd
    ..Default::default()
};

// Disable compression for speed
let config = DatabaseConfig {
    compression: CompressionType::None,
    ..Default::default()
};
```

---

### Configuration Methods

```rust
pub fn open_with_config(path: &str, config: DatabaseConfig) -> Result<BrowserDB, DBError>
```

Opens database with custom configuration.

**Examples:**
```rust
// High-performance configuration
let perf_config = DatabaseConfig {
    cache_size: 512 * 1024 * 1024,  // 512MB cache
    max_file_size: 128 * 1024 * 1024, // Large files
    compression: CompressionType::LZ4, // Fast compression
    enable_wal: false,                 // Disable WAL for speed
    auto_compact: true,
    max_background_jobs: 8,
};

let db = BrowserDB::open_with_config("high_perf.bdb", perf_config)?;

// Memory-efficient configuration
let memory_config = DatabaseConfig {
    cache_size: 16 * 1024 * 1024,    // Small cache
    max_file_size: 32 * 1024 * 1024, // Small files
    compression: CompressionType::Zstandard, // High compression
    enable_wal: true,
    auto_compact: true,
    max_background_jobs: 2,
};

let db = BrowserDB::open_with_config("memory_efficient.bdb", memory_config)?;
```

---

## 🚨 Error Handling

### DBError

```rust
pub enum DBError {
    NotFound,
    CorruptionDetected,
    IOError(String),
    InvalidArgument(String),
    OutOfMemory,
    DiskFull,
    PermissionDenied,
    ModeSwitchFailed,
    MigrationFailed,
    PoolExhausted,
}
```

**Error Categories:**

#### Not Found Errors

```rust
DBError::NotFound
```

**Meaning:** Requested key or file doesn't exist.

**Examples:**
```rust
match db.get(b"user:999") {
    Ok(Some(value)) => println!("Found: {:?}", value),
    Ok(None) => println!("User not found"), // This case
    Err(DBError::NotFound) => println!("Key doesn't exist"),
    Err(e) => eprintln!("Other error: {}", e),
}
```

#### Corruption Errors

```rust
DBError::CorruptionDetected
```

**Meaning:** Data integrity violation detected.

**Recovery:**
```rust
match db.get(b"critical:data") {
    Err(DBError::CorruptionDetected) => {
        eprintln!("Data corruption detected!");
        
        // Attempt automatic repair
        if let Err(repair_error) = db.repair() {
            eprintln!("Repair failed: {}", repair_error);
            // Restore from backup
            restore_from_backup("backup.bak")?;
        } else {
            println!("Repair successful");
        }
    },
    other => return other,
}
```

#### I/O Errors

```rust
DBError::IOError(String)
```

**Meaning:** File system or I/O operation failed.

**Examples:**
```rust
match db.put(b"key", b"value") {
    Err(DBError::IOError(msg)) => {
        if msg.contains("No space left") {
            eprintln!("Disk full!");
            // Clean up old files or switch to Ultra mode
            cleanup_old_files()?;
        } else if msg.contains("Permission denied") {
            eprintln!("Permission error!");
            // Check file permissions
        } else {
            eprintln!("I/O error: {}", msg);
        }
    },
    other => return other,
}
```

#### Memory Errors

```rust
DBError::OutOfMemory
```

**Meaning:** Insufficient memory for operation.

**Recovery:**
```rust
match db.put(b"large_key", &large_data) {
    Err(DBError::OutOfMemory) => {
        eprintln!("Out of memory!");
        
        // Reduce cache size
        let current_cache = db.get_cache_size()?;
        db.resize_cache(current_cache / 2)?;
        
        // Retry operation
        db.put(b"large_key", &large_data)?;
    },
    other => return other,
}
```

#### Mode Operation Errors

```rust
DBError::ModeSwitchFailed
DBError::MigrationFailed
```

**Meaning:** Mode switching or data migration failed.

**Recovery:**
```rust
match db.switch_to_persistent() {
    Err(DBError::ModeSwitchFailed) => {
        eprintln!("Mode switch failed!");
        
        // Get failure details
        let failure_info = db.get_switch_failure_info()?;
        eprintln!("Failure reason: {}", failure_info.reason);
        
        // Attempt rollback
        db.rollback_mode_switch()?;
    },
    other => return other,
}
```

### Error Recovery

#### Automatic Repair

```rust
pub fn repair(&self) -> Result<(), DBError>
```

Attempts to repair corrupted database.

**Examples:**
```rust
// Proactive repair
let validation_result = db.validate()?;
if !validation_result.is_valid {
    println!("Found issues, attempting repair...");
    
    match db.repair() {
        Ok(()) => println!("Repair successful"),
        Err(repair_error) => {
            eprintln!("Repair failed: {}", repair_error);
            
            // Fall back to backup
            restore_from_backup("backup.bak")?;
        }
    }
}
```

---

## 📊 Performance Monitoring

### Performance Statistics

```rust
pub struct PerformanceStats {
    pub reads_per_second: f64,
    pub writes_per_second: f64,
    pub cache_hit_rate: f64,
    pub average_query_time_ms: f64,
    pub p99_query_time_ms: f64,
    pub memory_usage_mb: f64,
    pub disk_usage_mb: f64,
    pub cache_usage_mb: f64,
}
```

**Methods:**

```rust
pub fn get_performance_stats(&self) -> Result<PerformanceStats, DBError>
```

Retrieves current performance metrics.

**Examples:**
```rust
let stats = db.get_performance_stats()?;

println!("=== Performance Statistics ===");
println!("Reads: {:.0} ops/sec", stats.reads_per_second);
println!("Writes: {:.0} ops/sec", stats.writes_per_second);
println!("Cache hit rate: {:.1}%", stats.cache_hit_rate * 100.0);
println!("Average query time: {:.2}ms", stats.average_query_time_ms);
println!("Memory usage: {:.1}MB", stats.memory_usage_mb);
```

---

### Memory Statistics

```rust
pub struct MemoryStats {
    pub heap_usage: usize,
    pub cache_usage: usize,
    pub cache_hit_rate: f64,
    pub total_allocations: u64,
    pub peak_usage: usize,
}
```

**Methods:**

```rust
pub fn get_memory_stats(&self) -> Result<MemoryStats, DBError>
```

Retrieves detailed memory usage information.

**Examples:**
```rust
let memory_stats = db.get_memory_stats()?;

println!("=== Memory Usage ===");
println!("Heap: {:.1}MB", memory_stats.heap_usage as f64 / 1024.0 / 1024.0);
println!("Cache: {:.1}MB", memory_stats.cache_usage as f64 / 1024.0 / 1024.0);
println!("Cache hit rate: {:.1}%", memory_stats.cache_hit_rate * 100.0);
println!("Total allocations: {}", memory_stats.total_allocations);

// Alert on high memory usage
if memory_stats.heap_usage > 100 * 1024 * 1024 {
    println!("⚠️ High memory usage detected");
    db.force_gc()?;
}
```

---

### Cache Management

```rust
pub fn resize_cache(&self, new_size: usize) -> Result<(), DBError>
```

Adjusts cache size dynamically.

**Examples:**
```rust
// Increase cache for better performance
let current_cache = db.get_cache_size()?;
let new_cache_size = current_cache * 2;
db.resize_cache(new_cache_size)?;

// Decrease cache to save memory
let memory_stats = db.get_memory_stats()?;
if memory_stats.heap_usage > 200 * 1024 * 1024 { // 200MB
    let reduced_cache = current_cache / 2;
    db.resize_cache(reduced_cache)?;
    println!("Cache reduced to {:.1}MB", reduced_cache as f64 / 1024.0 / 1024.0);
}
```

---

```rust
pub fn get_cache_size(&self) -> Result<usize, DBError>
```

Gets current cache size in bytes.

---

### Performance Optimization

#### Query Timing

```rust
pub struct Timer {
    start_time: std::time::Instant,
}

impl Timer {
    pub fn start() -> Timer {
        Timer {
            start_time: std::time::Instant::now(),
        }
    }
    
    pub fn elapsed(&self) -> std::time::Duration {
        self.start_time.elapsed()
    }
}
```

**Examples:**
```rust
let timer = Timer::start();

// Perform query
let result = db.get(b"user:123")?;
let duration = timer.elapsed();

if duration.as_millis() > 100 {
    println!("⚠️ Slow query detected: {}ms", duration.as_millis());
}

// Or use the built-in timer
let timer = db.start_timer();
let _ = db.get(b"key")?;
let duration = timer.elapsed();
```

#### Batch Performance

```rust
fn benchmark_batch_operations(db: &BrowserDB, operation_count: usize) -> Result<f64, Box<dyn Error>> {
    let timer = Timer::start();
    
    let mut batch = db.new_batch();
    for i in 0..operation_count {
        batch.put(&format!("key:{}", i).into_bytes(), 
                  &format!("value:{}", i).into_bytes());
        
        // Commit every 1000 operations for optimal performance
        if i % 1000 == 0 {
            batch.commit()?;
            batch = db.new_batch();
        }
    }
    batch.commit()?;
    
    let duration = timer.elapsed().as_secs_f64();
    let throughput = operation_count as f64 / duration;
    
    println!("Batch operations: {:.0} ops/sec", throughput);
    Ok(throughput)
}
```

---

## 💾 Backup and Recovery

### Backup Operations

```rust
pub struct Backup {
    path: std::path::PathBuf,
    metadata: BackupMetadata,
}

pub struct BackupMetadata {
    pub created_at: std::time::SystemTime,
    pub source_path: std::path::PathBuf,
    pub file_count: usize,
    pub total_size: usize,
    pub checksum: String,
}
```

#### Create Backup

```rust
pub fn create_backup(&self, backup_path: &str) -> Result<Backup, DBError>
```

Creates a backup of the current database.

**Parameters:**
- `backup_path`: Path for backup file

**Returns:**
- `Ok(Backup)`: Backup object with metadata
- `Err(DBError)`: Error code

**Examples:**
```rust
// Simple backup
let backup = db.create_backup("backup_20240115.bak")?;
println!("Backup created: {} files, {:.1}MB", 
         backup.metadata.file_count,
         backup.metadata.total_size as f64 / 1024.0 / 1024.0);

// Wait for backup completion (for large databases)
backup.wait_for_completion()?;

// Create timestamped backup
let timestamp = std::time::SystemTime::now()
    .duration_since(std::time::UNIX_EPOCH)?.as_secs();
let backup_name = format!("backup_{}.bak", timestamp);
let backup = db.create_backup(&backup_name)?;
```

---

#### Restore from Backup

```rust
pub fn restore_from_backup(backup_path: &str, target_path: &str) -> Result<BrowserDB, DBError>
```

Restores database from backup file.

**Parameters:**
- `backup_path`: Path to backup file
- `target_path`: Path for restored database

**Returns:**
- `Ok(BrowserDB)`: Restored database instance
- `Err(DBError)`: Error code

**Examples:**
```rust
// Restore to new location
let restored_db = BrowserDB::restore_from_backup("backup.bak", "restored_db.bdb")?;

// Verify restored data
let user_count = restored_db.prefix(b"user:")?.count()?;
println!("Restored {} users", user_count);

// Restore in-place (overwrite existing)
std::fs::copy("backup.bak", "original.bdb")?;
let original_db = BrowserDB::open("original.bdb")?;
```

---

### Incremental Backup

```rust
pub fn create_incremental_backup(&self, backup_path: &str, last_backup: Option<&Backup>) -> Result<Backup, DBError>
```

Creates incremental backup since last backup.

**Parameters:**
- `backup_path`: Path for incremental backup
- `last_backup`: Previous backup to compare against

**Examples:**
```rust
// First full backup
let full_backup = db.create_backup("full_backup.bak")?;

// Later incremental backup
let incremental_backup = db.create_incremental_backup("incremental_1.bak", Some(&full_backup))?;

println!("Incremental backup: {} changed files", incremental_backup.metadata.file_count);
```

---

### Backup Validation

```rust
pub fn validate_backup(&self, backup_path: &str) -> Result<bool, DBError>
```

Validates backup file integrity.

**Examples:**
```rust
let backup_path = "backup_20240115.bak";

match db.validate_backup(backup_path) {
    Ok(true) => println!("✅ Backup is valid"),
    Ok(false) => {
        println!("❌ Backup is corrupted");
        // Remove corrupted backup
        std::fs::remove_file(backup_path)?;
    },
    Err(error) => eprintln!("Backup validation error: {}", error),
}
```

---

## 🔗 Rust FFI Bindings

### Core Types

#### BrowserDB

Main database interface.

```rust
pub struct BrowserDB {
    internal: *mut browserdb_sys::BrowserDB,
}
```

**Safety:** All database operations are thread-safe for read operations, exclusive access required for writes.

---

### Function Bindings

#### Database Lifecycle

```rust
#[no_mangle]
pub extern "C" fn browserdb_open(path: *const c_char) -> *mut BrowserDB
```

Low-level C interface for database opening.

```rust
#[no_mangle]
pub extern "C" fn browserdb_close(db: *mut BrowserDB)
```

Closes database and frees resources.

---

#### Operations

```rust
#[no_mangle]
pub extern "C" fn browserdb_put(
    db: *mut BrowserDB, 
    key: *const u8, 
    key_len: usize, 
    value: *const u8, 
    value_len: usize
) -> c_int
```

Low-level put operation.

**Returns:**
- `0`: Success
- `-1`: Error (use `browserdb_get_error()` to get details)

---

#### Error Handling

```rust
#[no_mangle]
pub extern "C" fn browserdb_get_error() -> *const c_char
```

Gets last error message as C string.

**Examples (C):**
```c
#include "browserdb.h"

int main() {
    BrowserDB* db = browserdb_open("test.bdb");
    if (!db) {
        const char* error = browserdb_get_error();
        fprintf(stderr, "Failed to open database: %s\n", error);
        return 1;
    }
    
    const char* key = "test_key";
    const char* value = "test_value";
    
    int result = browserdb_put(db, (unsigned char*)key, strlen(key), 
                              (unsigned char*)value, strlen(value));
    
    if (result != 0) {
        const char* error = browserdb_get_error();
        fprintf(stderr, "Put failed: %s\n", error);
    }
    
    browserdb_close(db);
    return 0;
}
```

---

### WebAssembly Integration

For web usage, BrowserDB provides WebAssembly bindings.

#### JavaScript API

```javascript
// Import WebAssembly module
import init, { BrowserDB } from './browserdb_wasm.js';

async function main() {
    // Initialize WASM module
    await init();
    
    // Open database
    const db = await BrowserDB.open('webapp.bdb');
    
    // Store data
    await db.put('user:123', JSON.stringify({
        name: 'Alice',
        email: 'alice@example.com'
    }));
    
    // Retrieve data
    const userData = await db.get('user:123');
    const user = JSON.parse(new TextDecoder().decode(userData));
    console.log('User:', user.name);
    
    // Query operations
    const allUsers = await db.prefix('user:');
    for await (const [key, value] of allUsers) {
        console.log('Found user:', new TextDecoder().decode(value));
    }
}

main().catch(console.error);
```

#### TypeScript Definitions

```typescript
// browserdb.d.ts
export class BrowserDB {
    static open(path: string): Promise<BrowserDB>;
    
    put(key: Uint8Array, value: Uint8Array): Promise<void>;
    get(key: Uint8Array): Promise<Uint8Array | null>;
    delete(key: Uint8Array): Promise<void>;
    exists(key: Uint8Array): Promise<boolean>;
    
    range(start: Uint8Array, end: Uint8Array): Promise<RangeIterator>;
    prefix(prefix: Uint8Array): Promise<PrefixIterator>;
    
    close(): Promise<void>;
}

export interface RangeIterator {
    [Symbol.asyncIterator](): AsyncIterator<[Uint8Array, Uint8Array]>;
}

export interface PrefixIterator {
    [Symbol.asyncIterator](): AsyncIterator<[Uint8Array, Uint8Array]>;
    count(): Promise<number>;
}
```

---

### Browser Extension Integration

For browser extensions, native messaging is used:

#### Extension Background Script

```javascript
// background.js
const browser = chrome.runtime;

// Native messaging setup
browser.runtime.onMessage.addListener((request, sender, sendResponse) => {
    switch (request.action) {
        case 'openDatabase':
            openDatabase(request.path).then(sendResponse);
            return true; // Keep message channel open
            
        case 'putData':
            putData(request.key, request.value).then(sendResponse);
            return true;
            
        case 'getData':
            getData(request.key).then(sendResponse);
            return true;
    }
});

async function openDatabase(path) {
    return new Promise((resolve, reject) => {
        const port = chrome.runtime.connectNative('browserdb');
        
        port.onMessage.addListener((response) => {
            if (response.success) {
                resolve(response.db_id);
            } else {
                reject(new Error(response.error));
            }
        });
        
        port.postMessage({
            action: 'open',
            path: path
        });
    });
}
```

#### Native Host Manifest

```json
{
    "name": "browserdb",
    "description": "BrowserDB Native Host",
    "path": "/usr/local/bin/browserdb_native",
    "type": "stdio",
    "allowed_origins": [
        "chrome-extension://your-extension-id/"
    ]
}
```

---

<div align="center">

**[⬅️ Back to Developer Guide](DEVELOPER_GUIDE.md)** | **[🏠 README](README.md)** | **[📚 User Manual](USER_MANUAL.md)**

Complete API reference for all BrowserDB functionality!

</div>