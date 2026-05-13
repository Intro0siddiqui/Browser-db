# 👤 BrowserDB User Manual

Complete guide to using BrowserDB effectively in your applications.

## 📋 Table of Contents

1. [Getting Started](#getting-started)
2. [Core Data Tables](#core-data-tables)
3. [Performance Optimization](#performance-optimization)
4. [Error Handling](#error-handling)
5. [Best Practices](#best-practices)

---

## 🚀 Getting Started

### Database Creation and Opening

```rust
use browserdb::BrowserDB;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Open or create a database directory
    let db = BrowserDB::open("my_app_data")?;
    
    Ok(())
}
```

### Database Modes

BrowserDB supports two primary modes:
- **Persistent Mode** (Default): Disk-backed storage using LSM-Trees and WAL.
- **Ultra Mode**: Pure in-memory `HashMap` storage for maximum speed but no persistence.

You can switch modes at runtime:
```rust
db.set_mode(browserdb::DatabaseMode::Ultra)?;
```

---

## 🗄️ Core Data Tables

BrowserDB organizes data into specialized tables. Each table provides a type-safe API.

### Available Tables
- `history()`: Browsing history
- `cookies()`: HTTP Cookies
- `cache()`: Web resources
- `localstore()`: Origin-based key-value data with indexing
- `settings()`: General application preferences

### Basic Operations

#### 1. History Table
```rust
use browserdb::HistoryEntry;

db.history().insert(&HistoryEntry {
    timestamp: 1234567890,
    url: "https://example.com".to_string(),
    url_hash: 987654321,
    title: "Example Site".to_string(),
    visit_count: 5,
})?;

let entry = db.history().get(987654321)?;
```

#### 2. LocalStore Table (Indexed)
```rust
use browserdb::LocalStoreEntry;

let entry = LocalStoreEntry {
    origin_hash: 111222,
    key: "user_prefs".to_string(),
    value: "{\"theme\": \"dark\"}".to_string(),
};

// Insert with secondary index on the 'value' field
db.localstore().insert_with_index(&entry, &["value"])?;

// Query using the index
let results = db.localstore().query()
    .value_eq("{\"theme\": \"dark\"}".to_string())
    .execute()?;
```

#### 3. Settings Table
```rust
db.settings().set("app_version", "1.0.0")?;
let version = db.settings().get("app_version")?;
```

---

## ⚡ Performance Optimization

### 1. Batch Operations
For `localstore`, you can use `insert_with_index` which performs atomic updates to both the primary data and indices.

### 2. MemTable Tuning
Adjust `max_memtable_size_mb` in `browserdb.toml` to balance memory usage and disk I/O. Larger memtables reduce flush frequency but increase memory consumption.

---

## 🚨 Error Handling

Most operations return `Result<T, Box<dyn std::error::Error>>`. Always handle errors to prevent data inconsistency.

```rust
if let Err(e) = db.history().insert(&entry) {
    eprintln!("Failed to save history: {}", e);
}
```

---

## ✅ Best Practices

1. **Graceful Exit**: BrowserDB attempts to flush the MemTable on drop. Ensure your application shuts down cleanly to guarantee data persistence.
2. **Key Hashing**: Use consistent hashing for URLs and domains to ensure efficient lookups in history and cookie tables.
3. **Directory Permissions**: Ensure the application has read/write access to the database directory.

---

<div align="center">

**[⬅️ Back to Quick Start](QUICK_START.md)** | **[🏠 README](README.md)** | **[🛠️ Developer Guide](DEVELOPER_GUIDE.md)**

Master these patterns to build robust, high-performance applications!

</div>
