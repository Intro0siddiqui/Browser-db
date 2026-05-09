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
use browserdb::{BrowserDB, HistoryEntry};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Open or create a database at the specified path
    let db = BrowserDB::open("my_app.bdb")?;
    
    Ok(())
}
```

### Database Modes

BrowserDB automatically manages data in **Persistent Mode** (disk-backed) by default. It uses an LSM-Tree structure to ensure high-speed writes and crash recovery.

---

## 🗄️ Core Data Tables

BrowserDB organizes data into specialized tables for common browser use cases. Each table provides a type-safe API.

### Available Tables
- `history()`: Browsing history
- `cookies()`: HTTP Cookies
- `cache()`: Web resources
- `localstore()`: LocalStorage data
- `settings()`: Application preferences

### Basic CRUD Operations

#### 1. History Table
```rust
use browserdb::HistoryEntry;

// Insert
db.history().insert(&HistoryEntry {
    timestamp: 1234567890,
    url_hash: 987654321,
    title: "Rust Lang".to_string(),
    visit_count: 5,
})?;

// Get
if let Some(entry) = db.history().get(987654321)? {
    println!("Visited: {}", entry.title);
}
```

#### 2. Cookies Table
```rust
use browserdb::CookieEntry;

let mut cookie = CookieEntry::new(
    12345, // Domain Hash
    "session_id".to_string(),
    "xyz-token".to_string(),
    1700000000 // Expiry
);
cookie.set_secure();
cookie.set_httponly();

db.cookies().insert(&cookie)?;
```

#### 3. Settings Table (Key-Value)
```rust
// Set preference
db.settings().set("theme", "dark")?;

// Get preference
if let Some(theme) = db.settings().get("theme")? {
    println!("Theme: {}", theme);
}
```

---

## ⚡ Performance Optimization

### 1. Use the Right Tool
- **Raw Core Tables:** Use for high-frequency logging, caching, and history (700k+ writes/sec).

### 2. Persistence
BrowserDB uses an LSM-Tree which buffers writes in memory (MemTable). Data is automatically flushed to disk when:
- The MemTable reaches a size threshold.
- The database object is dropped (program exit).

**Warning:** Ensure your program exits gracefully to guarantee the final flush.

---

## 🚨 Error Handling

All operations return a standard `Result`.

```rust
match db.history().insert(&entry) {
    Ok(_) => println!("Saved!"),
    Err(e) => eprintln!("Database error: {}", e),
}
```

Common errors:
- **IO Errors:** Disk full, permission denied.
- **Serialization Errors:** Failed to encode/decode entry.

---

## ✅ Best Practices

1. **Key Management:** For `localstore` and `settings`, use consistent key naming (e.g., `app:window:width`).
2. **Thread Safety:** `BrowserDB` is thread-safe. Wrap it in `Arc<BrowserDB>` to share across threads.

---

<div align="center">

**[⬅️ Back to Quick Start](QUICK_START.md)** | **[🏠 README](README.md)** | **[🛠️ Developer Guide](DEVELOPER_GUIDE.md)**

Master these patterns to build robust, high-performance applications!

</div>