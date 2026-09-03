# 🔧 BrowserDB API Reference

Complete API documentation for all BrowserDB functions and types.

## 📋 Table of Contents

1. [Core Database API](#core-database-api)
2. [Tables and CRUD](#tables-and-crud)
3. [Query and Iteration](#query-and-iteration)
4. [Configuration](#configuration)
5. [Performance and Stats](#performance-and-stats)
6. [C/FFI Bindings](#cffi-bindings)
7. [Upcoming Features](#upcoming-features)

---

## 🗄️ Core Database API

### BrowserDB

Main database orchestrator supporting sharded storage containers (multi-tenancy) and global or container-specific table handles.

#### Constructors

```rust
pub fn open<P: AsRef<Path>>(path: P) -> Result<Self, Box<dyn std::error::Error>>
```

Opens an existing database or creates a new one at the specified directory path, acquiring an exclusive process-level lock via `fs2`.

```rust
pub fn open_without_locking<P: AsRef<Path>>(path: P) -> Result<Self, Box<dyn std::error::Error>>
```

Opens the database without acquiring an exclusive process-level lock.

#### Multi-Tenant Containers

```rust
pub fn container(&self, name: &str) -> Result<Arc<Container>, Box<dyn std::error::Error>>
```

Gets or creates an isolated storage container (tenant) within the database directory. Container names are sanitized (alphanumeric, `_`, `-`) to prevent path traversal.

#### Operations

```rust
pub fn set_mode(&self, mode: DatabaseMode) -> Result<(), Box<dyn std::error::Error>>
```

Switches all default container storage between `Persistent` (LSM-Tree + WAL) and `Ultra` (In-memory) modes.

```rust
pub fn wipe(&self) -> Result<(), Box<dyn std::error::Error>>
```

Clears all data from all tables across the default container.

```rust
pub fn stats(&self) -> Result<DatabaseStats, Box<dyn std::error::Error>>
```

Returns detailed database statistics.

---

## 📊 Tables and CRUD

BrowserDB uses specialized tables for different data types.

### History Table

Access via `db.history()` or `container.history()`.

```rust
pub fn insert(&self, entry: &HistoryEntry) -> Result<(), Box<dyn std::error::Error>>
pub fn insert_with_ttl(&self, entry: &HistoryEntry, ttl_ms: u64) -> Result<(), Box<dyn std::error::Error>>
pub fn increment(&self, url_hash: u128, delta: i64) -> Result<(), Box<dyn std::error::Error>>
pub fn get(&self, url_hash: u128) -> Result<Option<HistoryEntry>, Box<dyn std::error::Error>>
pub fn hot_search(&self, query: &str, limit: usize) -> Result<Vec<HistoryEntry>, Box<dyn std::error::Error>>
pub fn count(&self) -> Result<usize, Box<dyn std::error::Error>>
pub fn wipe_domain(&self, domain: &str) -> Result<usize, Box<dyn std::error::Error>>
```

### Bookmarks Table

Access via `db.bookmarks()` or `container.bookmarks()`.

```rust
pub fn insert(&self, entry: &BookmarkEntry) -> Result<(), Box<dyn std::error::Error>>
pub fn get_all(&self) -> Result<Vec<BookmarkEntry>, Box<dyn std::error::Error>>
pub fn delete(&self, url_hash: u128) -> Result<(), Box<dyn std::error::Error>>
pub fn count(&self) -> Result<usize, Box<dyn std::error::Error>>
```

### Cookies Table

Access via `db.cookies()` or `container.cookies()`.

```rust
pub fn insert(&self, entry: &CookieEntry) -> Result<(), Box<dyn std::error::Error>>
pub fn get(&self, domain_hash: u128, name: &str) -> Result<Option<CookieEntry>, Box<dyn std::error::Error>>
pub fn get_by_domain(&self, domain_hash: u128) -> Result<Vec<CookieEntry>, Box<dyn std::error::Error>>
pub fn get_all(&self) -> Result<Vec<CookieEntry>, Box<dyn std::error::Error>>
pub fn delete(&self, domain_hash: u128, name: &str) -> Result<(), Box<dyn std::error::Error>>
pub fn count(&self) -> Result<usize, Box<dyn std::error::Error>>
```

### Cache Table

Access via `db.cache()` or `container.cache()`.

```rust
pub fn insert(&self, entry: &CacheEntry) -> Result<(), Box<dyn std::error::Error>>
pub fn get(&self, url_hash: u128) -> Result<Option<CacheEntry>, Box<dyn std::error::Error>>
pub fn count(&self) -> Result<usize, Box<dyn std::error::Error>>
```

### LocalStore Table

Access via `db.localstore()` or `container.localstore()`.

```rust
pub fn insert(&self, entry: &LocalStoreEntry) -> Result<(), Box<dyn std::error::Error>>
pub fn insert_with_index(&self, entry: &LocalStoreEntry, index_fields: &[&str]) -> Result<(), Box<dyn std::error::Error>>
pub fn get(&self, origin_hash: u128, key: &str) -> Result<Option<LocalStoreEntry>, Box<dyn std::error::Error>>
pub fn get_by_origin(&self, origin_hash: u128) -> Result<Vec<LocalStoreEntry>, Box<dyn std::error::Error>>
pub fn remove(&self, origin_hash: u128, key: &str) -> Result<(), Box<dyn std::error::Error>>
pub fn clear_origin(&self, origin_hash: u128) -> Result<(), Box<dyn std::error::Error>>
pub fn count(&self) -> Result<usize, Box<dyn std::error::Error>>
pub fn query(&self) -> QueryBuilder
```

### BinaryStore Table

Access via `db.binarystore()` or `container.binarystore()`. Generic byte-slice KV interface.

```rust
pub fn put(&self, key: Vec<u8>, value: Vec<u8>) -> Result<(), Box<dyn std::error::Error>>
pub fn get(&self, key: &[u8]) -> Result<Option<Vec<u8>>, Box<dyn std::error::Error>>
pub fn delete(&self, key: &[u8]) -> Result<(), Box<dyn std::error::Error>>
pub fn scan_prefix(&self, prefix: &[u8]) -> Result<Vec<(Vec<u8>, Vec<u8>)>, Box<dyn std::error::Error>>
pub fn all_entries(&self) -> Result<Vec<(Vec<u8>, Vec<u8>)>, Box<dyn std::error::Error>>
pub fn count(&self) -> Result<usize, Box<dyn std::error::Error>>
```

### Settings Table

Access via `db.settings()` or `container.settings()`.

```rust
pub fn set(&self, key: &str, value: &str) -> Result<(), Box<dyn std::error::Error>>
pub fn get(&self, key: &str) -> Result<Option<String>, Box<dyn std::error::Error>>
pub fn count(&self) -> Result<usize, Box<dyn std::error::Error>>
```

---

## 🔍 Query and Iteration

### QueryBuilder (LocalStore)

Fluent API for querying LocalStore data with optional index utilization.

```rust
db.localstore().query()
    .prefix(prefix_bytes)
    .value_eq("target_value".to_string())
    .limit(10)
    .execute()?;
```

---

## ⚙️ Configuration

### BrowserDBConfig

Loaded from `browserdb.toml` in the database directory.

```rust
pub struct BrowserDBConfig {
    pub lsm_tree: LsmTreeConfig,
    pub heatmap: HeatmapConfig,
}

pub struct LsmTreeConfig {
    pub max_level0_files: usize,    // Default: 4
    pub max_memtable_size_mb: usize, // Default: 20
    pub level_size_thresholds_mb: Vec<usize>, // Default: [10, 100, 1000...]
}

pub struct HeatmapConfig {
    pub max_entries: usize,     // Default: 10000
    pub hot_threshold: u32,     // Default: 10
    pub decay_factor: f64,      // Default: 0.95
}
```

---

## 📈 Performance and Stats

### DatabaseStats

```rust
pub fn stats(&self) -> Result<DatabaseStats, Box<dyn std::error::Error>>
```

Returns current database metrics.

```rust
pub struct DatabaseStats {
    pub total_entries: u64,
    pub history_entries: u64,
    pub cookie_entries: u64,
    pub cache_entries: u64,
    pub localstore_entries: u64,
    pub settings_entries: u64,
    pub memory_usage_mb: u64,
    pub disk_usage_mb: u64,
}
```

---

## 🔗 C/FFI Bindings

BrowserDB provides a stable C-compatible interface for integration with other languages.

```rust
#[no_mangle]
pub extern "C" fn browserdb_open(path: *const c_char) -> *mut BrowserDB;

#[no_mangle]
pub extern "C" fn browserdb_close(db: *mut BrowserDB);

#[no_mangle]
pub extern "C" fn browserdb_history_insert(
    db: *mut BrowserDB,
    url: *const c_char,
    title: *const c_char,
    visit_count: u32
) -> c_int;

#[no_mangle]
pub extern "C" fn browserdb_history_get_title(
    db: *mut BrowserDB,
    url_hash_low: u64,
    url_hash_high: u64
) -> *mut c_char;

#[no_mangle]
pub extern "C" fn browserdb_free_string(s: *mut c_char);
```

---

## 🚀 Upcoming Features

The following features are currently in development and are NOT yet available in the stable API:

- **Automatic Repair**: `db.repair()` for corrupted files.
- **Backup & Restore**: `db.create_backup()` and `BrowserDB::restore_from_backup()`.
- **WebAssembly**: Direct browser-side WASM compilation.

---

<div align="center">

**[⬅️ Back to Developer Guide](DEVELOPER_GUIDE.md)** | **[🏠 README](README.md)** | **[📚 User Manual](USER_MANUAL.md)**

Complete API reference for all BrowserDB functionality!

</div>
