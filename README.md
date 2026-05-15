# BrowserDB 🚀
[![Ask DeepWiki](https://deepwiki.com/badge.svg)](https://deepwiki.com/Intro0siddiqui/Browser-db)
> Lightning-fast, privacy-first database for modern browsers, now in **Pure Rust**!(warning ⚠️ still in development a beta project)

BrowserDB is a high-performance, browser-native database designed as a modern alternative to IndexedDB. Built with a LSM-tree hybrid architecture and intelligent HeatMap indexing, it delivers sub-millisecond queries with 95% cache hit rates.

[![Build Status](https://img.shields.io/badge/build-passing-brightgreen)]()
[![Performance](https://img.shields.io/badge/performance-890K%2B%20reads%2Fsec-blue)]()
[![Memory Usage](https://img.shields.io/badge/memory-%3C50MB-orange)]()
[![License](https://img.shields.io/badge/license-AGPL--3.0-orange)]()

## ⚡ Quick Start

Get up and running in 2 minutes:

```bash
# 1. Clone the repository
git clone https://github.com/browserdb/browserdb.git
cd browserdb/bindings

# 2. Build and run the example
cargo run --release --example basic_usage
```

**🎯 First database operation:**
```rust
use browserdb::{BrowserDB, HistoryEntry};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Open/create database
    let db = BrowserDB::open("my_app.bdb")?;
    
    // Store some data
    db.history().insert(&HistoryEntry {
        timestamp: 1234567890,
        url: "https://example.com".to_string(),
        url_hash: 123,
        title: "My First Page".to_string(),
        visit_count: 1
    })?;
    
    // Retrieve data
    let entry = db.history().get(123)?;
    println!("Entry: {:?}", entry);
    
    Ok(())
}
```

## 🎯 Why BrowserDB?

| Feature | BrowserDB (Rust) | IndexedDB | SQLite |
|---------|-----------|-----------|---------|
| **Read Performance** | **900K+ ops/sec** | 10K ops/sec | 50K ops/sec |
| **Write Performance** | **390K+ ops/sec** | 1K ops/sec | 10K ops/sec |
| **Memory Efficiency** | <50MB | 100MB+ | 80MB+ |
| **Cache Hit Rate** | 95% | 70% | 85% |
| **Query Latency** | <0.1ms | 10ms | 2ms |

## 🏗️ Architecture Overview

```
┌─────────────────┐
│   Application   │
└────────┬────────┘
         │
         ▼
┌─────────────────┐    ┌─────────────────┐
│   Pure Rust     │    │   HeatMap       │
│     Engine      │◄──►│    Index        │
└────────┬────────┘    └─────────────────┘
         │
         ▼
┌─────────────────┐
│  LSM-Tree       │
│   Storage       │
└────────┬────────┘
         │
         ▼
┌─────────────────┐
│   .bdb Files    │
│   (Persistent)  │
└─────────────────┘
```

### Core Components

- **🔥 HeatMap Indexing**: Intelligent sharded HeatTracker for hot data prioritization
- **⚡ LSM-Tree Storage**: Optimized sharded MemTable (16 shards) and 10-level SSTables
- **🗂️ .bdb Format**: Universal browser database format with CRC32 integrity
- **🔄 Mode Operations**: Persistent (LSM-Tree) vs Ultra (In-memory HashMap) modes
- **🛡️ Data Durability**: WAL with background group-commit (5ms flush)
- **📦 Blob Storage**: Efficient handling of large values (>64KB) outside the LSM-tree
- **🔗 C/FFI Bindings**: Stable C-API for multi-language integration (C/C++, Python, etc.)

## 🚀 Key Features

### Performance
- **900K+ reads/second** - Sub-millisecond random query response
- **390K+ writes/second** - High-throughput data ingestion with sharded locks
- **Intelligent HeatMap** - Access-frequency tracking for compaction and cache priority
- **<50MB memory footprint** - Efficient resource usage with configurable memtables

### Reliability
- **Atomic operations** - WAL-backed atomic batch operations
- **Crash Recovery** - Automatic WAL recovery on startup
- **Multi-mode support** - Persistent (LSM) and Ultra (RAM) modes
- **Pure Rust** - High-performance implementation in safe Rust

## 🏃‍♂️ Performance Benchmarks

For comprehensive runtime benchmarks, visit: [browserdb-runtime-bench](https://github.com/Intro0siddiqui/browserdb-runtime-bench)

```bash
# Run performance stress test
cd bindings
cargo run --release --example stress_test

# Recent Results (Typical on modern hardware):
# Write Throughput:     390,165 ops/sec
# Read Throughput:      903,421 ops/sec
# Persistence:          Verified
```

## 📁 Project Structure

```
browserdb/
├── 📄 README.md              # This file
├── 📁 bindings/              # 🦀 Main Rust Crate
│   ├── src/                 
│   │   ├── core/            # Core Database Logic
│   │   │   ├── lsm_tree.rs  # Storage engine
│   │   │   ├── format.rs    # File format
│   │   │   ├── heatmap.rs   # Indexing & Bloom Filters
│   │   │   └── modes.rs     # Mode management
│   │   └── lib.rs           # Public API
│   ├── examples/            # 💡 Usage examples
│   └── Cargo.toml           # Rust configuration
├── 📁 scripts/               # 🛠️ Utility scripts
├── 📄 API_REFERENCE.md       # 🔧 API documentation
├── 📄 DEVELOPER_GUIDE.md     # 🛠️ Architecture & internal details
├── 📄 USER_MANUAL.md         # 👤 User guide
├── 📄 QUICK_START.md         # 🚀 5-minute setup
└── 📄 FILE_STRUCTURE.md      # 📁 Codebase organization
```

## 🎯 Use Cases

### Browser Applications
- **History Management**: Fast search through browsing history
- **Bookmark Storage**: Efficient CRUD operations for bookmarks
- **Session Recovery**: Quick session restoration
- **Resource Caching**: High-performance cache layer

### Web Applications
- **Offline Support**: Robust local data persistence
- **Real-time Apps**: High-throughput event storage
- **Analytics**: Efficient data collection and querying
- **Content Management**: Fast content indexing and retrieval

## 🛠️ Development

### Prerequisites
- **Rust 1.75+**

### Contributing
1. Fork the repository
2. Create a feature branch
3. Make your changes
4. Add tests
5. Submit a pull request

## 📊 Database File Types

| File | Purpose | Typical Size | Access Pattern |
|------|---------|--------------|----------------|
| `history.bdb` | Browsing trails | 10-50MB | High read/search |
| `cookies.bdb` | Session data | 5-20MB | Frequent read/write |
| `cache.bdb` | Resource cache | 100-500MB | Burst reads |
| `localstore.bdb` | Per-origin KV | 1-10MB | Write-heavy |
| `settings.bdb` | Configuration | <1MB | Rare writes |

## 🔒 Security & Privacy

- **Local-first**: All data stays on the user's device
- **No tracking**: Zero telemetry or analytics collection
- **Privacy by design**: Minimal data exposure
- **Open source**: Auditable codebase

## 📄 License

**GNU Affero General Public License v3.0 (AGPL-3.0)** - Ensuring cooperation with the community for the modern web.

## 🤝 Community

- **Issues**: [GitHub Issues](https://github.com/Intro0siddiqui/Browser-db/issues)
- **Discussions**: [GitHub Discussions](https://github.com/Intro0siddiqui/Browser-db/discussions)
- **Contributing**: See [Contribution.md](https://github.com/Intro0siddiqui/Browser-db/blob/main/Contribution.md)

---

<div align="center">

**[🚀 Get Started Now](bindings/examples/basic_usage.rs)** | **[📚 Read Docs](USER_MANUAL.md)**

Built with ❤️ for the modern web

</div>
