# 🛠️ BrowserDB Developer Guide

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

BrowserDB is a Pure Rust LSM-tree hybrid engine designed for browser-native performance:

1. **Performance First**: 900k+ reads/sec and 390k+ writes/sec via sharded locking.
2. **Crash Consistency**: WAL-backed operations with group commits.
3. **Multi-Mode**: Seamless switching between Persistent (LSM) and Ultra (In-memory) modes.
4. **Intelligent Indexing**: HeatTracker-driven compaction and access optimization.

### High-Level Architecture

```
┌─────────────────────────────────────────────────────────────────┐
│                        Application Layer                        │
│                (Rust Crate / C-FFI / WASM Support)              │
└─────────────────────────────┬───────────────────────────────────┘
                              │
┌─────────────────────────────┴───────────────────────────────────┐
│                       Pure Rust Engine                          │
│  ┌──────────────┐  ┌──────────────┐  ┌──────────────┐          │
│  │  BrowserDB   │  │  HeatTracker │  │   Mode       │          │
│  │ Coordinator  │  │  (Sharded)   │  │   Switcher   │          │
│  └──────────────┘  └──────────────┘  └──────────────┘          │
│         │                    │                    │           │
│  ┌──────┴──────────────┐     │                    │           │
│  │     LSM-Tree        │◄────┘                    │           │
│  │ (10 Levels + WAL)   │                          │           │
│  └──────────┬──────────┘                          │           │
│             │                │                    │           │
│  ┌──────────▼──────────┐     │                                │
│  │   Blob Storage      │     │                                │
│  │  (Large Objects)    │     │                                │
│  └─────────────────────┘     │                                │
│                              │                                │
│  ┌───────────────────────────┼────────────────────────────────┤
│  │                    File System                             │
│  │  ┌──────────────┐ ┌──────────────┐ ┌──────────────┐      │
│  │  │  .wal Files  │ │  .sst Files  │ │  .blob Files │      │
│  │  │ (Group Commit)│ │ (Mmap Read)  │ │ (Direct I/O) │      │
│  │  └──────────────┘ └──────────────┘ └──────────────┘      │
│  └───────────────────────────────────────────────────────────┘
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

### 4. HeatTracker
A sharded access monitoring system that tracks "heat" (access frequency) for keys. It uses a decay mechanism to ensure that only currently relevant data is considered "hot".

---

## 📦 Internal Subsystems

### Blob Storage (`blob_log.rs`)
To prevent LSM-tree bloat, values larger than 64KB are automatically redirected to the Blob Storage. The LSM-tree stores a small `BlobPointer` instead of the actual data, keeping SSTables compact and efficient for scanning.

### C/FFI Layer (`ffi.rs`)
BrowserDB exports a stable C-compatible API, allowing it to be used from C, C++, Python, or Node.js. It handles string conversions and memory management across the FFI boundary safely.

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
