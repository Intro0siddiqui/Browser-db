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
- `history()`: Browsing history (supports TTL, counters via `increment`, and `hot_search`)
- `bookmarks()`: Bookmark entries management
- `cookies()`: HTTP Cookies management with domain-based queries
- `cache()`: Web resources caching
- `localstore()`: Origin-based key-value data with secondary indexing and `QueryBuilder`
- `binarystore()`: Low-level raw byte key-value store with prefix scanning
- `settings()`: General application preferences

### Multi-Tenant Storage Containers
BrowserDB supports isolated storage containers (tenants) within a single database path:
```rust
let tenant_a = db.container("user_alice")?;
tenant_a.history().insert(&entry)?;

let tenant_b = db.container("user_bob")?;
let alice_history = tenant_a.history().count()?;
let bob_history = tenant_b.history().count()?;
```

### Table Examples

#### 1. History Table & Features
```rust
use browserdb::HistoryEntry;

// Standard insert
db.history().insert(&HistoryEntry {
    timestamp: 1234567890,
    url: "https://example.com".to_string(),
    url_hash: 987654321,
    title: "Example Site".to_string(),
    visit_count: 5,
})?;

// Insert with Time-To-Live (TTL in milliseconds)
db.history().insert_with_ttl(&entry, 3600_000)?; // Expires in 1 hour

// Atomic merge counter increment (no read-modify-write needed)
db.history().increment(987654321, 1)?;

// Heat-ranked search
let top_matches = db.history().hot_search("example", 10)?;
```

#### 2. Bookmarks Table
```rust
use browserdb::BookmarkEntry;

db.bookmarks().insert(&BookmarkEntry {
    timestamp: 1234567890,
    url: "https://rust-lang.org".to_string(),
    url_hash: 42,
    title: "Rust Programming Language".to_string(),
})?;

let all_bookmarks = db.bookmarks().get_all()?;
db.bookmarks().delete(42)?;
```

#### 3. LocalStore Table (Indexed)
```rust
use browserdb::LocalStoreEntry;

let entry = LocalStoreEntry {
    origin_hash: 111222,
    key: "user_prefs".to_string(),
    value: "{\"theme\": \"dark\"}".to_string(),
};

// Insert with secondary index on the 'value' field
db.localstore().insert_with_index(&entry, &["value"])?;

// Fast Query using the index
let results = db.localstore().query()
    .value_eq("{\"theme\": \"dark\"}".to_string())
    .execute()?;
```

#### 4. BinaryStore Table (Raw KV)
```rust
// Store arbitrary binary key-value data
db.binarystore().put(b"session:1001".to_vec(), b"auth_token_bytes".to_vec())?;

let token = db.binarystore().get(b"session:1001")?;

// Fast prefix scan
let sessions = db.binarystore().scan_prefix(b"session:")?;
```

#### 5. Settings Table
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
