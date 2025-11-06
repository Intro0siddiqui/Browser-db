# 🚀 BrowserDB Quick Start Guide

Get up and running with BrowserDB in 5 minutes!

## 📋 Prerequisites

Before we start, ensure you have the following installed:

```bash
# Check versions
zig --version    # Should be 0.13.0+
rustc --version  # Should be 1.75+
cargo --version  # Should be 1.75+
```

### Installing Missing Dependencies

**Zig (Core Engine):**
```bash
# Linux/macOS
curl -L https://github.com/ziglang/zig/releases/download/0.13.0/zig-linux-x86_64-0.13.0.tar.xz | \
tar -xJ && sudo mv zig-linux-x86_64-0.13.0 /opt/zig
sudo ln -sf /opt/zig/zig /usr/local/bin/zig

# Windows (PowerShell)
Invoke-WebRequest -Uri "https://github.com/ziglang/zig/releases/download/0.13.0/zig-windows-x86_64-0.13.0.zip" -OutFile "zig.zip"
Expand-Archive -Path "zig.zip" -DestinationPath "C:\zig"
$env:PATH += ";C:\zig"
```

**Rust (FFI Bindings):**
```bash
# Install Rust
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
source ~/.cargo/env
rustup toolchain install stable
```

## 🏗️ Installation

### Step 1: Clone the Repository
```bash
git clone https://github.com/browserdb/browserdb.git
cd browserdb
```

### Step 2: Build the Core Engine
```bash
cd core
zig build
```

**Expected output:**
```
Build Summary: 6/6 steps successful
```

### Step 3: Verify Installation
```bash
# Run the test suite
zig build test

# Expected output:
# All tests passed!
# - Unit tests: 45/45 ✅
# - Integration tests: 12/12 ✅
# - Performance tests: 8/8 ✅
```

### Step 4: Build Rust Bindings
```bash
cd ../bindings
cargo build --release

# Expected output:
# Compiling browserdb v0.1.0
# Finished release [optimized] target(s) in 45.2s
```

## 🎯 Your First Database

### Basic Usage Example

Create a file called `hello_browserdb.rs`:

```rust
use browserdb::*;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // 1. Open/create a database
    let db = BrowserDB::open("hello_world.bdb")?;
    
    // 2. Store some data
    db.put("user:alice", "Alice Smith")?;
    db.put("user:bob", "Robert Jones")?;
    db.put("settings:theme", "dark")?;
    
    // 3. Retrieve data
    let alice = db.get("user:alice")?;
    let theme = db.get("settings:theme")?;
    
    println!("User: {:?}", alice);
    println!("Theme: {:?}", theme);
    
    // 4. Query multiple records
    let users: Vec<_> = db.range("user:", "user~")?
        .filter_map(|r| r.ok())
        .map(|r| String::from_utf8_lossy(&r).to_string())
        .collect();
    
    println!("All users: {:?}", users);
    
    Ok(())
}
```

### Running Your First Program

```bash
cd examples
cargo run --bin hello_browserdb
```

**Expected output:**
```
User: Some(b"Alice Smith")
Theme: Some(b"dark")
All users: ["Alice Smith", "Robert Jones"]
```

## 📊 Common Operations

### CRUD Operations

```rust
use browserdb::*;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let db = BrowserDB::open("app_data.bdb")?;
    
    // CREATE: Store data
    db.put("key1", "value1")?;
    db.put("key2", "value2")?;
    
    // READ: Retrieve data
    if let Some(value) = db.get("key1")? {
        println!("Found: {}", String::from_utf8_lossy(&value));
    }
    
    // UPDATE: Overwrite existing data
    db.put("key1", "updated_value")?;
    
    // DELETE: Remove data
    db.delete("key2")?;
    
    Ok(())
}
```

### Batch Operations (Faster!)

```rust
// Insert multiple records efficiently
let mut batch = db.new_batch();
batch.put("user:1", "Alice");
batch.put("user:2", "Bob");
batch.put("user:3", "Charlie");
batch.commit()?;

// Query multiple records
let users: Vec<_> = db.prefix("user:")?
    .map(|r| String::from_utf8_lossy(&r.1).to_string())
    .collect();
```

### Advanced Queries

```rust
// Range queries (ordered by key)
let range: Vec<_> = db.range("a", "z")?
    .map(|r| r.map(|r| String::from_utf8_lossy(&r.1).to_string()))
    .collect()?;

// Prefix searches (for structured data)
let settings: Vec<_> = db.prefix("settings:")?
    .map(|r| r.map(|r| String::from_utf8_lossy(&r.1).to_string()))
    .collect()?;

// Exact key searches
let user = db.get("user:123")?;
```

## ⚡ Performance Tips

### 1. Use Batch Operations
```rust
// ❌ Slow: Individual puts
for i in 0..1000 {
    db.put(&format!("key:{}", i), &format!("value:{}", i))?;
}

// ✅ Fast: Batch commit
let mut batch = db.new_batch();
for i in 0..1000 {
    batch.put(&format!("key:{}", i), &format!("value:{}", i))?;
}
batch.commit()?;
```

### 2. Choose the Right Mode
```rust
// For frequent writes (cache, session data)
let db = BrowserDB::open_ultra("temp_cache.bdb")?;  // RAM mode

// For persistent data (user data, settings)
let db = BrowserDB::open("persistent.bdb")?;        // Disk mode
```

### 3. Use Appropriate Key Structure
```rust
// ✅ Good: Structured keys for efficient prefix queries
db.put("user:123:profile", json!({...}))?;
db.put("user:123:settings", json!({...}))?;

// Later: Fast prefix query
let user_data: Vec<_> = db.prefix("user:123:")?.collect();

// ❌ Avoid: Unstructured keys
db.put("user123profile", ...)?;
db.put("user123settings", ...)?;
```

## 🔧 Configuration Options

### Database Modes

```rust
// Ultra mode: All in RAM, fastest access
let ultra_db = BrowserDB::open_ultra("cache.bdb")?;

// Persistent mode: Disk-backed with cache
let persistent_db = BrowserDB::open("data.bdb")?;

// Custom configuration
let config = BrowserDBConfig {
    cache_size: 256 * 1024 * 1024, // 256MB cache
    max_file_size: 64 * 1024 * 1024, // 64MB per file
    compression: CompressionType::LZ4,
    enable_wal: true, // Write-ahead logging
};

let db = BrowserDB::open_with_config("configured.bdb", config)?;
```

### Performance Tuning

```rust
// For read-heavy workloads
let config = BrowserDBConfig {
    cache_size: 512 * 1024 * 1024, // Larger cache
    max_file_size: 32 * 1024 * 1024, // Smaller files
    ..Default::default()
};

// For write-heavy workloads  
let config = BrowserDBConfig {
    cache_size: 128 * 1024 * 1024, // Smaller cache
    max_file_size: 128 * 1024 * 1024, // Larger files
    enable_wal: true,
    compression: CompressionType::None, // Skip compression for speed
};
```

## 🚨 Troubleshooting

### Common Issues

**1. Build Failures**
```bash
# Clean build
cd core && rm -rf zig-cache && zig build clean && zig build

# Check Zig version
zig version  # Should be 0.13.0+
```

**2. Permission Errors**
```bash
# Make scripts executable
chmod +x scripts/*.sh

# Check write permissions
touch test.bdb && rm test.bdb
```

**3. Performance Issues**
```bash
# Check cache hit rates
let stats = db.get_statistics()?;
println!("Cache hit rate: {}%", stats.cache_hit_rate * 100.0);

// Adjust cache size if hit rate < 90%
```

**4. Memory Usage**
```bash
# Monitor memory usage
let memory_stats = db.get_memory_statistics()?;
println!("Memory usage: {}MB", memory_stats.heap_usage / 1024 / 1024);
```

### Getting Help

1. **Check Documentation**: See [USER_MANUAL.md](USER_MANUAL.md)
2. **Run Diagnostics**: `zig build test --diagnostic`
3. **Check Logs**: Look for `.log` files in your project directory
4. **GitHub Issues**: [Report bugs](https://github.com/browserdb/browserdb/issues)

## 🎯 Next Steps

### What You've Learned
- ✅ Installing BrowserDB
- ✅ Creating and opening databases
- ✅ Basic CRUD operations
- ✅ Performance optimization
- ✅ Configuration options

### What's Next
- **📚 User Manual**: Deep dive into all features
- **🛠️ Developer Guide**: Architecture and customization
- **🔧 API Reference**: Complete function documentation
- **📁 File Structure**: Understanding the codebase

### Quick Reference
```rust
// Open database
let db = BrowserDB::open("mydb.bdb")?;

// CRUD operations
db.put(key, value)?;
let value = db.get(key)?;
db.delete(key)?;

// Queries
let items = db.prefix("prefix:")?.collect::<Result<Vec<_>, _>>()?;

// Batch operations
let mut batch = db.new_batch();
batch.put(key, value);
batch.commit()?;
```

---

<div align="center">

**[⬅️ Back to README](README.md)** | **[📚 User Manual](USER_MANUAL.md)** | **[🛠️ Developer Guide](DEVELOPER_GUIDE.md)**

🎉 **Congratulations! You're now ready to build amazing applications with BrowserDB!**

</div>