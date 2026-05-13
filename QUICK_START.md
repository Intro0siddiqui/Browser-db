# 🚀 BrowserDB Quick Start Guide

Get up and running with BrowserDB in 2 minutes!

## 📋 Prerequisites

Before we start, ensure you have Rust installed:

```bash
# Check versions
rustc --version  # Should be 1.75+
cargo --version  # Should be 1.75+
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

### Step 3: Run Tests (Optional)
```bash
cargo test
```

## 🎯 Your First Database

### Basic Usage Example

```rust
use browserdb::{BrowserDB, HistoryEntry};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // 1. Open/create a database directory
    let db = BrowserDB::open("my_db")?;
    
    // 2. Store some data using the type-safe API
    db.history().insert(&HistoryEntry {
        timestamp: 1234567890,
        url: "https://rust-lang.org".to_string(),
        url_hash: 123,
        title: "Rust Programming Language".to_string(),
        visit_count: 1
    })?;
    
    // 3. Retrieve data
    if let Some(entry) = db.history().get(123)? {
        println!("Found: {:?}", entry.title);
    }
    
    Ok(())
}
```

## 🏃‍♂️ Performance Check

Run the built-in stress test to see BrowserDB in action:

```bash
cargo run --release --example stress_test
```

---

<div align="center">

**[⬅️ Back to README](README.md)** | **[📚 User Manual](USER_MANUAL.md)** | **[🛠️ Developer Guide](DEVELOPER_GUIDE.md)**

🎉 **Congratulations! You're now ready to build amazing applications with BrowserDB!**

</div>
