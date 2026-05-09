# 📁 BrowserDB File Structure Guide

Understanding how BrowserDB is organized helps developers navigate, contribute, and extend the codebase.

## 🏗️ Project Architecture Overview

```
BrowserDB/
├── 🦀 Core Engine (Rust)          # High-performance database engine
├── 💡 Examples                    # Usage examples and tutorials
├── 🛠️ Scripts                     # Build and deployment automation
└── 📚 Documentation               # User and developer guides
```

---

## 📂 Source Directory (`bindings/src/`)

The heart of BrowserDB - written in Pure Rust for maximum performance and safety.

### 📄 Core Source Files (`src/core/`)

#### `lib.rs` - Library Entry Point
**Purpose:** Main public API and library initialization.

**Key Responsibilities:**
- Database lifecycle management (open, close)
- Table accessors (history, cookies, etc.)
- Mode switching integration

**Key Functions:**
```rust
pub fn open(path: impl AsRef<Path>) -> Result<Self, Error>
pub fn history(&self) -> HistoryTable<'_>
```

#### `core/lsm_tree.rs` - Storage Engine
**Purpose:** Implements the Log-Structured Merge-Tree storage architecture using `BTreeMap` and memory-mapped files.

**Key Responsibilities:**
- MemTable (in-memory write buffer, BTreeMap-backed)
- SSTable (Sorted String Table) management
- Disk flushing and recovery
- Binary search across SSTable files

**Key Structures:**
```rust
pub struct LSMTree {
    pub memtable: RwLock<MemTable>,
    pub levels: Vec<RwLock<Vec<Arc<SSTable>>>>,
}

pub struct SSTable {
    pub mmap: Mmap,
    pub bloom_filter: Option<BloomFilter>,
    pub index: Vec<IndexEntry>,
}
```

#### `core/format.rs` - File Format & I/O
**Purpose:** Defines the universal `.bdb` file format serialization/deserialization.

**Key Responsibilities:**
- Binary format specification
- CRC32 integrity checking
- Varint encoding/decoding

**File Format Structure:**
```
┌─────────────────────────────────────────────────────────────┐
│ Header (Magic, Version, Timestamp)                         │
├─────────────────────────────────────────────────────────────┤
│ Entry Data Stream                                          │
│ ├── Type (1 byte)                                          │
│ ├── Key Length (Varint) + Key Data                         │
│ ├── Value Length (Varint) + Value Data                     │
│ ├── Timestamp (8 bytes)                                    │
│ └── CRC32 (4 bytes)                                        │
└─────────────────────────────────────────────────────────────┘
```

#### `core/modes.rs` - Mode Management
**Purpose:** Handles database mode switching (Persistent vs Ultra).

**Key Responsibilities:**
- `PersistentMode`: Disk-backed LSM trees.
- `UltraMode`: In-memory `HashMap` storage.
- Atomic mode transitions.

#### `core/heatmap.rs` - Cache System & Bloom Filters
**Purpose:** Intelligent caching and optimization.

**Key Responsibilities:**
- Bloom Filters for SSTable lookups (probabilistic checking)
- Heat tracking for hot data detection

---

---

## 💡 Examples Directory (`bindings/examples/`)

**Purpose:** Usage examples and tutorials

#### `basic_usage.rs`
- Simple KV operations
- Database creation

#### `stress_test.rs`
- Performance benchmarking script
- Verifies 700k+ ops/sec throughput

---

## 📚 Documentation Structure

**Purpose:** User and developer documentation

```
docs/
├── USER_MANUAL.md                 # Complete user guide  
├── DEVELOPER_GUIDE.md             # Architecture and development
├── FILE_STRUCTURE.md              # This file
├── API_REFERENCE.md               # Function documentation
└── QUICK_START.md                 # 5-minute setup guide
```

---

<div align="center">

**[⬅️ Back to Quick Start](QUICK_START.md)** | **[🏠 Project README](README.md)** | **[📚 User Manual](USER_MANUAL.md)**

Understanding the structure makes development and contribution efficient!

</div>