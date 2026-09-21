# 🚀 ZawraDB Quick Start Guide

Get up and running with ZawraDB in 2 minutes!

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
git clone https://github.com/zawradb/zawradb.git
cd zawradb/bindings
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
use zawradb::{ZawraDB, HistoryEntry, LocalStoreEntry};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // 1. Open/create a database directory
    let db = ZawraDB::open("my_db")?;
    
    // 2. Store data in the History table
    db.history().insert(&HistoryEntry {
        timestamp: 1234567890,
        url: "https://rust-lang.org".to_string(),
        url_hash: 123,
        title: "Rust Programming Language".to_string(),
        visit_count: 1
    })?;
    
    // 3. Increment counters atomically via LSM merge operator
    db.history().increment(123, 1)?;

    // 4. Retrieve data
    if let Some(entry) = db.history().get(123)? {
        println!("Found: {:?}, visits: {}", entry.title, entry.visit_count);
    }

    // 5. Use Multi-tenant isolated storage containers
    let tenant = db.container("user_tenant_1")?;
    tenant.settings().set("theme", "dark")?;
    
    Ok(())
}
```

## 🏃‍♂️ Performance Check

Run the built-in stress test to see ZawraDB in action:

```bash
cargo run --release --example stress_test
```

---

<div align="center">

**[⬅️ Back to README](README.md)** | **[📚 User Manual](USER_MANUAL.md)** | **[🛠️ Developer Guide](DEVELOPER_GUIDE.md)**

🎉 **Congratulations! You're now ready to build amazing applications with ZawraDB!**

</div>
