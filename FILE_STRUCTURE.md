# 📁 BrowserDB File Structure Guide

Understanding how BrowserDB is organized helps developers navigate, contribute, and extend the codebase.

## 🏗️ Project Architecture Overview

```
BrowserDB/
├── 🦀 Core Engine & Bindings (Rust) # High-performance database engine in `bindings/`
├── 💡 Examples                      # Usage examples in `bindings/examples/` and root `examples/`
├── 🛠️ Scripts                       # Build automation scripts in `scripts/`
└── 📚 Documentation                 # Documentation guides in repository root
```

---

## 📂 Source Directory (`bindings/src/`)

The heart of BrowserDB - written in 100% Pure Rust for maximum performance and safety.

### 📄 Core Source Files (`bindings/src/`)

#### `lib.rs` - Main Library Entry Point & Table APIs
**Purpose:** Main public API, table structs (`HistoryTable`, `BookmarksTable`, `CookiesTable`, `CacheTable`, `LocalStoreTable`, `BinaryStoreTable`, `SettingsTable`), multi-tenant container management, and `QueryBuilder`.

**Key Responsibilities:**
- Database lifecycle management (`open`, `open_without_locking`, `wipe`, `stats`)
- Container isolation (`db.container("name")`)
- Table handles & fluent queries

#### `ffi.rs` - C-Compatible FFI Layer
**Purpose:** C ABI interface for FFI access from Bun, Deno, C, C++, Node.js, and Python.

### 📄 Core Storage Subsystems (`bindings/src/core/`)

#### `core/lsm_tree.rs` - Storage Engine & LSM-Tree
**Purpose:** Implements 10-level LSM-Tree, sharded MemTable (16 shards), size-tiered compaction cascades, TTL filtering, `increment` merge operators, streaming merge iterators (`self_cell`), and index lookups.

#### `core/blob_log.rs` - WiscKey Blob Log Storage
**Purpose:** Manages out-of-band blob log files (`.blob`) for values larger than 64KB, keeping SSTables lean and compaction fast.

#### `core/wal.rs` - Crash-Resilient Write-Ahead Log
**Purpose:** Sequential append-only WAL manager with background group-commits (flushing every 5ms or 32KB buffer) and graceful crash recovery.

#### `core/format.rs` - SSTable File Format & Checksums
**Purpose:** SSTable binary format encoder/decoder, 47-byte header (`BDBFileHeader`), 60-byte footer (`BDBFileFooter`), 4KB block CRC32 checksums, and prefix compression.

#### `core/heatmap.rs` - Access Tracking & Bloom Filters
**Purpose:** 32-shard heat tracking system with decay for hot-data compaction prioritization and Bloom filters for fast SSTable search filtering.

#### `core/modes.rs` - Execution Modes & Transitions
**Purpose:** Orchestrates mode switching between `PersistentMode` (LSM-Tree + WAL) and `UltraMode` (in-memory HashMap).

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
.
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