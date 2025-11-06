# 📁 BrowserDB File Structure Guide

Understanding how BrowserDB is organized helps developers navigate, contribute, and extend the codebase.

## 🏗️ Project Architecture Overview

```
BrowserDB/
├── 🎯 Core Engine (Zig)           # High-performance database engine
├── 🔗 FFI Bindings (Rust)         # Language bindings for integration  
├── 💡 Examples                    # Usage examples and tutorials
├── 🛠️ Scripts                     # Build and deployment automation
└── 📚 Documentation               # User and developer guides
```

---

## 📂 Core Directory (`core/`)

The heart of BrowserDB - written in Zig for maximum performance.

### 📄 Core Source Files (`src/core/`)

#### `browserdb.zig` - Main Database Engine
**Purpose:** Central orchestrator that coordinates all subsystems

**Key Responsibilities:**
- Database lifecycle management (open, close, create, delete)
- Mode switching between Persistent and Ultra modes
- Request routing to appropriate subsystems
- Transaction coordination and ACID compliance
- Error handling and recovery

**Key Functions:**
```zig
pub fn open(path: []const u8) !BrowserDB
pub fn put(db: *BrowserDB, key: []const u8, value: []const u8) !void
pub fn get(db: *BrowserDB, key: []const u8) !?[]const u8
pub fn delete(db: *BrowserDB, key: []const u8) !void
pub fn range(db: *BrowserDB, start: []const u8, end: []const u8) !RangeIterator
pub fn switchMode(db: *BrowserDB, mode: DatabaseMode) !void
```

**Integration Points:**
- LSM-Tree for storage operations
- HeatMap for cache management
- File I/O through bdb_format
- Mode operations coordination

---

#### `lsm_tree.zig` - Storage Engine
**Purpose:** Implements the Log-Structured Merge-Tree storage architecture

**Key Responsibilities:**
- SSTable (Sorted String Table) management
- MemTable (in-memory write buffer)
- Compaction operations (size-tiered, tiered, leveled)
- Binary search across SSTable files
- Storage optimization and garbage collection

**Key Functions:**
```zig
pub fn put(lsm: *LSMTree, key: []const u8, value: []const u8) !void
pub fn get(lsm: *LSMTree, key: []const u8) !?[]const u8
pub fn flushMemTable(lsm: *LSMTree) !void
pub fn compact(lsm: *LSMTree, strategy: CompactionStrategy) !void
pub fn findSSTableFiles(lsm: *LSMTree, pattern: []const u8) ![]SSTableFile
```

**Storage Hierarchy:**
```
Memory (MemTable) → L0 SSTables → L1 SSTables → L2 SSTables ...
     ↓                    ↓              ↓
   Fast Writes      Compaction       Archival
```

**Key Components:**
- **SSTableFile:** Individual immutable sorted files
- **MemTable:** In-memory write buffer (default: 32MB)
- **CompactionEngine:** Merges and optimizes storage
- **BinarySearch:** Fast key lookup across SSTables

---

#### `bdb_format.zig` - File Format & I/O
**Purpose:** Defines and implements the universal .bdb file format

**Key Responsibilities:**
- .bdb file format specification and implementation
- CRC32 integrity checking and validation
- Compression/decompression (LZ77, LZ4, Zstandard)
- File I/O operations with streaming support
- Corruption detection and recovery

**Key Functions:**
```zig
pub fn openFile(path: []const u8) !BDBFile
pub fn readEntry(file: *BDBFile, offset: u64) !Entry
pub fn writeEntry(file: *BDBFile, entry: Entry) !u64
pub fn calculateCRC32(data: []const u8) u32
pub fn compress(algorithm: CompressionType, data: []const u8) ![]const u8
pub fn validateFile(file: *BDBFile) !ValidationResult
```

**File Format Structure:**
```
┌─────────────────────────────────────────────────────────────┐
│ Header (64 bytes)                                           │
│ ├── Magic: "BROWSERDB" (8 bytes)                          │
│ ├── Version: 1 (4 bytes)                                  │
│ ├── File size (8 bytes)                                   │
│ ├── Entry count (8 bytes)                                 │
│ ├── Header CRC32 (4 bytes)                                │
│ └── Reserved (32 bytes)                                   │
├─────────────────────────────────────────────────────────────┤
│ Entry Data Stream                                          │
│ ├── Key length (2 bytes) + Key data                      │
│ ├── Value length (4 bytes) + Value data                  │
│ ├── Entry CRC32 (4 bytes)                                │
│ └── Repeat for all entries                               │
├─────────────────────────────────────────────────────────────┤
│ Footer (32 bytes)                                          │
│ ├── Metadata hash (16 bytes)                             │
│ ├── Data size (8 bytes)                                  │
│ ├── File CRC32 (4 bytes)                                 │
│ └── Reserved (4 bytes)                                   │
└─────────────────────────────────────────────────────────────┘
```

**Compression Support:**
- **LZ77 (Zlib):** Balanced compression ratio, moderate speed
- **LZ4:** High performance, lower compression
- **Zstandard:** High compression ratio, medium speed
- **None:** Raw storage for maximum speed

---

#### `modes_operations.zig` - Mode Management
**Purpose:** Handles database mode switching and lifecycle operations

**Key Responsibilities:**
- Atomic mode transitions (Persistent ↔ Ultra)
- Data migration between modes
- Progress tracking for long operations
- Rollback capability for failed operations
- User notification system

**Key Functions:**
```zig
pub fn switchToPersistent(db: *BrowserDB) !ModeSwitchResult
pub fn switchToUltra(db: *BrowserDB) !ModeSwitchResult
pub fn migrateData(from_mode: DatabaseMode, to_mode: DatabaseMode) !void
pub fn getSwitchProgress(db: *BrowserDB) SwitchProgress
pub fn cancelModeSwitch(db: *BrowserDB) !void
```

**Mode Characteristics:**

| Feature | Persistent Mode | Ultra Mode |
|---------|----------------|------------|
| **Storage** | Disk + RAM cache | RAM only |
| **Durability** | Full ACID | Volatile |
| **Speed** | Fast (cached) | Instant |
| **Memory Usage** | <50MB | Unlimited* |
| **Use Case** | User data, settings | Cache, temp data |
| **Persistence** | Survives restart | Lost on restart |

**Mode Switching Process:**
1. **Preparation:** Validate target mode, check resources
2. **Migration:** Copy data between storage layers
3. **Validation:** Ensure data integrity after migration
4. **Activation:** Switch internal systems to new mode
5. **Cleanup:** Remove old data, optimize storage

---

#### `heatmap_indexing.zig` - Cache System
**Purpose:** Intelligent caching system with HeatMap indexing algorithm

**Key Responsibilities:**
- Hot data detection and prioritization
- Cache replacement optimization (95%+ hit rate)
- Performance monitoring and analytics
- Memory-efficient cache operations
- Adaptive caching strategies

**Key Functions:**
```zig
pub fn get(c: *HeatMapCache, key: []const u8) !?[]const u8
pub fn put(c: *HeatMapCache, key: []const u8, value: []const u8) !void
pub fn evict(c: *HeatMapCache, key: []const u8) !void
pub fn getHitRate(c: *HeatMapCache) f64
pub fn getMemoryStats(c: *HeatMapCache) MemoryStats
```

**HeatMap Algorithm:**
```
┌─────────────────────────────────────────────────────────────┐
│ Access Pattern Analysis                                     │
│ ┌─────────┐ ┌─────────┐ ┌─────────┐ ┌─────────┐            │
│ │  Hot    │ │ Warm    │ │  Cold   │ │ Frozen  │            │
│ │  95%    │ │  4%     │ │  0.9%   │ │  0.1%   │            │
│ │ hits    │ │ hits    │ │ hits    │ │ hits    │            │
│ └─────────┘ └─────────┘ └─────────┘ └─────────┘            │
│                                                             │
│ Eviction Priority: Frozen → Cold → Warm → Hot              │
└─────────────────────────────────────────────────────────────┘
```

**Cache Optimization Features:**
- **Frequency Tracking:** Counts actual access patterns
- **Recency Analysis:** Time since last access
- **Size Awareness:** Evict large items first when needed
- **Adaptive Sizing:** Automatically adjust cache size
- **Performance Metrics:** Real-time hit rate monitoring

---

### 📁 Test Directory (`tests/`)

**Purpose:** Comprehensive testing infrastructure

#### `lsm_tree_tests.zig`
- SSTable creation and management tests
- MemTable flush operations
- Compaction algorithm validation
- Binary search accuracy tests
- Performance benchmark tests

#### `bdb_format_tests.zig`
- File format validation tests
- CRC32 integrity verification
- Compression algorithm tests
- Corruption recovery tests
- File I/O stress tests

#### `modes_operations_tests.zig`
- Mode switching functionality
- Data migration verification
- Progress tracking tests
- Rollback mechanism validation
- Performance impact measurements

#### `heatmap_indexing_tests.zig`
- Cache hit rate validation
- Memory usage optimization tests
- Access pattern analysis tests
- Eviction algorithm correctness
- Performance under load

#### `performance_benchmarks.zig`
- Read/write throughput tests
- Memory usage profiling
- Cache hit rate benchmarks
- Mode switching performance
- Scalability tests

---

## 🔗 Bindings Directory (`bindings/`)

Rust FFI bindings for language integration.

### 📄 Binding Source Files (`src/`)

#### `lib.rs` - Library Entry Point
**Purpose:** Main FFI interface and library initialization

**Key Functions:**
```rust
pub struct BrowserDB {
    internal: *mut c_void,
}

impl BrowserDB {
    pub fn open(path: &str) -> Result<Self, Box<dyn Error>> {
        // Bridge Zig core with Rust
    }
    
    pub fn put(&self, key: &[u8], value: &[u8]) -> Result<(), Box<dyn Error>> {
        // Convert to C types, call Zig functions
    }
}
```

#### `ffi.rs` - Foreign Function Interface
**Purpose:** Low-level FFI bridge between Rust and Zig

**Key Functions:**
```rust
#[no_mangle]
pub extern "C" fn browserdb_open(path: *const c_char) -> *mut BrowserDB {
    // C-compatible interface for browser integration
}

#[no_mangle] 
pub extern "C" fn browserdb_put(db: *mut BrowserDB, key: *const u8, key_len: usize, value: *const u8, value_len: usize) -> c_int {
    // Raw C interface for maximum compatibility
}
```

#### `operations.rs` - High-Level Operations
**Purpose:** Rust-friendly wrappers around core functionality

**Key Functions:**
```rust
impl BrowserDB {
    pub fn get(&self, key: &[u8]) -> Result<Option<Vec<u8>>, DBError> {
        // User-friendly error handling
    }
    
    pub fn range(&self, start: &[u8], end: &[u8]) -> Result<RangeIterator, DBError> {
        // Iterator pattern for range queries
    }
}
```

#### `types.rs` - Type Definitions
**Purpose:** Shared type definitions and error types

**Key Types:**
```rust
pub enum CompressionType {
    None,
    LZ4,
    Zlib,
    Zstandard,
}

pub struct DatabaseConfig {
    pub cache_size: usize,
    pub max_file_size: usize,
    pub compression: CompressionType,
}

pub enum DBError {
    NotFound,
    CorruptionDetected,
    IOError(String),
    InvalidArgument(String),
}
```

---

## 💡 Examples Directory

**Purpose:** Usage examples and tutorials

#### `basic_usage.rs`
- Simple CRUD operations
- Database creation and opening
- Basic error handling
- Performance tips

#### Integration Examples (Future)
- `web_integration.html` - Browser integration
- `react_component.rsx` - React component example
- `nodejs_integration.js` - Node.js bindings

---

## 🛠️ Scripts Directory

**Purpose:** Build and deployment automation

#### `build.sh`
```bash
#!/bin/bash
# Automated build script
set -e

echo "Building BrowserDB..."

# Build core engine
cd core
zig build -Drelease-safe

# Build bindings
cd ../bindings
cargo build --release

echo "Build complete!"
```

---

## 📚 Documentation Structure

**Purpose:** User and developer documentation

```
docs/
├── DEPLOYMENT_MIGRATION_GUIDE.md  # Deployment and migration
├── USER_MANUAL.md                 # Complete user guide  
├── DEVELOPER_GUIDE.md             # Architecture and development
├── FILE_STRUCTURE.md              # This file
└── API_REFERENCE.md               # Function documentation
```

---

## 🔄 Integration Points

### Core Engine Integration
```
browserdb.zig (Coordinator)
    ├── lsm_tree.zig (Storage)
    ├── bdb_format.zig (Files)
    ├── modes_operations.zig (Modes)
    └── heatmap_indexing.zig (Cache)
```

### FFI Integration  
```
Rust Bindings → C ABI → Zig Core Engine
     ↓              ↓           ↓
JavaScript ← Browser APIs ← Database Ops
```

### File System Integration
```
.bdb Files → bdb_format.zig → lsm_tree.zig → browserdb.zig
    ↓            ↓            ↓            ↓
Disk I/O    Validation   Storage     Application
```

---

## 🎯 Developer Guidelines

### Code Organization Principles

1. **Single Responsibility:** Each file has one clear purpose
2. **Clear Interfaces:** Well-defined function signatures
3. **Error Handling:** Comprehensive error types and handling
4. **Performance:** Optimized for speed and memory efficiency
5. **Documentation:** Self-documenting with clear comments

### Contribution Workflow

1. **Understand the Architecture:** Read this guide first
2. **Identify the Right File:** Find the appropriate component
3. **Follow Patterns:** Maintain existing code patterns
4. **Add Tests:** Include comprehensive tests
5. **Update Documentation:** Keep docs in sync

### Performance Considerations

- **Memory:** All components optimize for <50MB usage
- **Speed:** Target sub-millisecond operations for hot data
- **Scalability:** Support millions of records efficiently
- **Reliability:** ACID compliance with corruption recovery

---

<div align="center">

**[⬅️ Back to Quick Start](QUICK_START.md)** | **[🏠 Project README](README.md)** | **[📚 User Manual](USER_MANUAL.md)**

Understanding the structure makes development and contribution efficient!

</div>