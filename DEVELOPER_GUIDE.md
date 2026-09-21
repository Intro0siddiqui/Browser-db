# 🛠️ ZawraDB Developer Guide

Architecture, implementation details, and guidelines for contributors.

## 📋 Table of Contents

1. [Architecture Overview](#architecture-overview)
2. [Core Components](#core-components)
3. [Internal Subsystems](#internal-subsystems)
4. [Development Setup](#development-setup)
5. [Contribution Guidelines](#contribution-guidelines)
6. [Performance Engineering](#performance-engineering)
7. [Testing Strategy](#testing-strategy)

---

## 🏗️ Architecture Overview

### System Design Philosophy

ZawraDB is a Pure Rust LSM-tree hybrid engine designed for browser-native performance:

1. **Performance First**: ~900k reads/sec and ~390k writes/sec via 16-shard MemTables and sharded heat tracking.
2. **Crash Consistency**: WAL-backed operations with background group commits (5ms / 32KB buffer).
3. **Multi-Tenant Model**: Isolated storage containers per tenant (`db.container("name")`).
4. **Multi-Mode**: Seamless switching between Persistent (LSM-Tree + WAL) and Ultra (RAM) modes.
5. **Advanced Engine Features**: Write-side secondary indexing, LSM merge operators (`increment`), TTL enforcement, and WiscKey Blob separation.

### High-Level Architecture

```
┌─────────────────────────────────────────────────────────────────┐
│                        Application Layer                        │
│                  (Rust Crate / C-FFI / WASM)                    │
└─────────────────────────────┬───────────────────────────────────┘
                              │
┌─────────────────────────────┴───────────────────────────────────┐
│                   ZawraDB Engine Core                         │
│  ┌──────────────┐  ┌──────────────┐  ┌────────────────────────┐  │
│  │ Multi-Tenant │  │  HeatTracker │  │     Process Lock       │  │
│  │ Containers   │  │ (32 Shards)   │  │ (`fs2` zawradb.lock) │  │
│  └──────────────┘  └──────────────┘  └────────────────────────┘  │
│         │                    │                    │             │
│  ┌──────┴──────────────┐     │                    │             │
│  │ 16-Shard MemTable   │◄────┘                    │             │
│  │ & 10-Level LSM-Tree │                          │             │
│  └──────────┬──────────┘                          │             │
│             │                │                    │             │
│  ┌──────────▼──────────┐     │                                  │
│  │   WiscKey BlobLog   │     │                                  │
│  │ (>64KB Value Direct)│     │                                  │
│  └─────────────────────┘     │                                  │
│                              │                                  │
│  ┌───────────────────────────┼──────────────────────────────────┤
│  │                    File System                               │
│  │  ┌──────────────┐ ┌──────────────┐ ┌──────────────┐        │
│  │  │  .wal Files  │ │  .sst Files  │ │  .blob Files │        │
│  │  │ (Group Commit)│ │ (Mmap Read)  │ │ (Direct I/O) │        │
│  │  └──────────────┘ └──────────────┘ └──────────────┘        │
│  └─────────────────────────────────────────────────────────────┘
└─────────────────────────────────────────────────────────────────┘
```

---

## 🔧 Core Components

### 1. Sharded MemTable
To minimize lock contention, the MemTable is sharded into 16 independent `BTreeMap` shards, each protected by its own `RwLock`. Keys are routed to shards via hash-based distribution.

### 2. LSM-Tree & Compaction
- **10 Levels**: Support for leveled storage (Level 0 to Level 9).
- **Background Compaction**: Triggered when Level 0 reaches `max_level0_files` (default: 4).
- **Heat-Aware**: The HeatTracker influences compaction priority, ensuring hot data is optimized first.

### 3. WAL Manager (Group Commit)
The Write-Ahead Log uses a background thread to perform "Group Commits" every 5ms or when the 32KB buffer is full, significantly reducing I/O latency for high-frequency writes.

### 4. HeatTracker & Hot Search
A sharded access monitoring system that tracks "heat" (access frequency) for keys. It uses 32 independent shards (`Vec<RwLock<HashMap>>`) and atomic decay to minimize lock contention. Heat values drive Level 0 compaction ordering and the `hot_search` API.

### 5. Multi-Tenant Storage Containers
Isolated storage environments within a single database path. Container names are sanitized to prevent directory traversal (`alphanumeric`, `_`, `-`), each managing its own WAL and table space.

### 6. Process-Level Multi-Process Locking
Multi-process coordination is enforced at initialization by acquiring an exclusive lock on `zawradb.lock` using the `fs2` crate. Non-locking access is supported via `open_without_locking`.

### 7. LSM Merge Operators & Time-To-Live (TTL)
- **Merge Operators (`EntryType::Increment`)**: Allows high-throughput counter updates without read-modify-write overhead. 64-bit deltas are aggregated during reads and compacted during LSM merges.
- **TTL Enforcement**: Support for expiration timestamps (`expires_at` in ms). Expired records are dynamically filtered during reads and permanently reclaimed during compaction and Ultra-mode purges.

### 8. Native Secondary Indexing
Supports secondary indexing for JSON/field lookups (`insert_with_index`). Write-side index entries are maintained with an `is_index` flag to prevent recursive indexing loops.

---

## 📦 Internal Subsystems

### Blob Storage (`blob_log.rs`)
To prevent LSM-tree bloat, values larger than 64KB are automatically redirected to the Blob Storage. The LSM-tree stores a small `BlobPointer` instead of the actual data, keeping SSTables compact and efficient for scanning.

### C/FFI Layer (`ffi.rs`)
ZawraDB exports a stable C-compatible API, allowing it to be used from C, C++, Python, or Node.js. It handles string conversions and memory management across the FFI boundary safely.

---

## 🛠️ Development Setup

### Prerequisites

```bash
# Required versions
Rust:   >= 1.75.0  
```

### Build System

```bash
cd bindings

# Debug build
cargo build

# Release build
cargo build --release

# Run tests
cargo test

# Run benchmarks
cargo run --release --example stress_test
```

---

## 🤝 Contribution Guidelines

### Code Style

- Use `rustfmt` for formatting.
- Use `clippy` for linting.
- Ensure all public structures are Serializable/Deserializable via `serde`.

### Pull Request Process

1. **Fork and Branch**: Create feature branch from `main`
2. **Implement**: Follow coding standards and add tests
3. **Verify**: Run `cargo test` and `cargo run --example stress_test`
4. **Submit**: Create pull request

---

## ⚡ Performance Engineering

### Performance Targets

| Metric | Measured (Release) |
|--------|--------|
| **Read Throughput** | 900K+ ops/sec |
| **Write Throughput** | 390K+ ops/sec |
| **Random Query Latency** | < 1µs |

---

## 🧪 Testing Strategy

### Stress Testing
Use the provided `stress_test` example to verify performance and WAL recovery:
```bash
cargo run --release --example stress_test
```

---

<div align="center">

**[⬅️ Back to User Manual](USER_MANUAL.md)** | **[📁 File Structure](FILE_STRUCTURE.md)** | **[🔧 API Reference](API_REFERENCE.md)**

Ready to contribute? Start with the [development setup](#development-setup)!

</div>
