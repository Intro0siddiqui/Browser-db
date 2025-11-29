# 🚀 BrowserDB Quick Start Guide

Get up and running with BrowserDB in 2 minutes!

## 📋 Prerequisites

Before we start, ensure you have Rust installed:

```bash
# Check versions
rustc --version  # Should be 1.75+
cargo --version  # Should be 1.75+
```

**Installing Rust:**
```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
source ~/.cargo/env
rustup toolchain install stable
```

## 🏗️ Installation

### Step 1: Clone the Repository
```bash
git clone https://github.com/browserdb/browserdb.git
cd browserdb/bindings
```

### Step 2: Build the Project
```bash
cargo build --release
```

**Expected output:**
```
Compiling browserdb v0.1.0
Finished release [optimized] target(s) in ...s
```

### Step 3: Run Tests (Optional)
```bash
cargo test
```

## 🎯 Your First Database

### Basic Usage Example

Create a file called `hello_browserdb.rs` in `examples/`:

```rust
use browserdb::{BrowserDB, HistoryEntry};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // 1. Open/create a database
    let db = BrowserDB::open("hello_world.bdb")?;
    
    // 2. Store some data using the type-safe API
    db.history().insert(&HistoryEntry {
        timestamp: 1234567890,
        url_hash: 123,
        title: "My First Page".to_string(),
        visit_count: 1
    })?;
    
    // 3. Retrieve data
    if let Some(entry) = db.history().get(123)? {
        println!("Found: {:?}", entry);
    } else {
        println!("Not found!");
    }
    
    Ok(())
}
```

### Running Your First Program

```bash
cargo run --example basic_usage
```

## 📊 Common Operations

### Using the SQL Subsystem

BrowserDB now supports a modular SQL layer for structured queries.

```rust
use browserdb::BrowserDB;
use std::sync::Arc;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let db = BrowserDB::open("sql_demo.bdb")?;
    let sql = Arc::new(db).sql();

    // Create Table
    sql.execute("CREATE TABLE users (id INT PRIMARY_KEY, name TEXT)")?;

    // Insert Data
    sql.execute("INSERT INTO users VALUES (1, 'Alice')")?;

    // Query Data
    let result = sql.execute("SELECT * FROM users WHERE id = 1")?;
    println!("Query Result: {}", result);

    Ok(())
}
```

## ⚡ Performance Tips

### 1. Use Raw Core for Speed
If you need maximum throughput (logs, cache, history), use the raw Rust API (`db.history().insert(...)`). It bypasses SQL parsing overhead.

### 2. Use SQL for Flexibility
If you need structured schemas and ease of use, use the SQL subsystem. It is still very fast (~250k ops/sec) but slower than raw core.

## 🚨 Troubleshooting

### Common Issues

**1. Build Failures**
```bash
# Clean build
cargo clean && cargo build --release
```

**2. Permission Errors**
Ensure the application has write permissions to the directory where `.bdb` files are created.

### Getting Help

1. **Check Documentation**: See [USER_MANUAL.md](USER_MANUAL.md)
2. **GitHub Issues**: [Report bugs](https://github.com/browserdb/browserdb/issues)

## 🎯 Next Steps

### What's Next
- **📚 User Manual**: Deep dive into all features
- **🛠️ Developer Guide**: Architecture and customization
- **🔧 API Reference**: Complete function documentation

---

<div align="center">

**[⬅️ Back to README](README.md)** | **[📚 User Manual](USER_MANUAL.md)** | **[🛠️ Developer Guide](DEVELOPER_GUIDE.md)**

🎉 **Congratulations! You're now ready to build amazing applications with BrowserDB!**

</div>