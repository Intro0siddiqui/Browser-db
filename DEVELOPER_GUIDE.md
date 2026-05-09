# 🛠️ BrowserDB Developer Guide

Architecture, implementation details, and guidelines for contributors.

## 📋 Table of Contents

1. [Architecture Overview](#architecture-overview)
2. [Core Components](#core-components)
3. [Development Setup](#development-setup)
4. [Contribution Guidelines](#contribution-guidelines)
5. [Performance Engineering](#performance-engineering)
6. [Testing Strategy](#testing-strategy)

---

## 🏗️ Architecture Overview

### System Design Philosophy

BrowserDB follows these core principles:

1. **Performance First**: Sub-millisecond queries (700k+ ops/sec) for hot data
2. **Memory Efficient**: <50MB footprint with intelligent caching
3. **Reliability**: ACID compliance with corruption recovery
4. **Simplicity**: Clear Pure Rust interfaces
5. **Simplicity**: No complex SQL parser overhead

### High-Level Architecture

```
┌─────────────────────────────────────────────────────────────────┐
│                        Application Layer                        │
│                 (Rust Crate / FFI / WASM)                       │
└─────────────────────────────┬───────────────────────────────────┘
                              │
┌─────────────────────────────┴───────────────────────────────────┐
│                       Pure Rust Engine                          │
│  ┌──────────────┐  ┌──────────────┐  ┌──────────────┐          │
│  │  BrowserDB   │  │  HeatMap     │  │   Modes &    │          │
│  │ Coordinator  │  │   Cache      │  │  Operations  │          │
│  └──────────────┘  └──────────────┘  └──────────────┘          │
│         │                    │                    │           │
│  ┌──────┴──────────────┐     │                    │           │
│  │     LSM-Tree        │     │                    │           │
│  │     Storage         │     │                    │           │
│  └─────────────────────┘     │                    │           │
│                              │                                │
│  ┌───────────────────────────┼────────────────────────────────┤
│  │                    File System                             │
│  │  ┌──────────────┐ ┌──────────────┐ ┌──────────────┐      │
│  │  │  .bdb Files  │ │  SSTables    │ │   Format     │      │
│  │  │  (Universal) │ │  (Storage)   │ │ (Serialization)│    │
│  │  └──────────────┘ └──────────────┘ └──────────────┘      │
│  └───────────────────────────────────────────────────────────┘
└─────────────────────────────────────────────────────────────────┘
```

### Data Flow

```
Write Operation:
App → BrowserDB → LSM-Tree → MemTable (BTreeMap) → (Flush) → SSTable

Read Operation:  
App → BrowserDB → HeatMap Cache → (Miss) → LSM-Tree → SSTables
```

---

## 🔧 Core Components

### 1. BrowserDB Coordinator (`lib.rs`)

**Purpose**: Central orchestrator managing all subsystems

**Key Responsibilities**:
- Database lifecycle management
- Table accessors (History, Cookies, etc.)
- Mode switching

**Purpose**: High-performance storage with optimal write/read balance

**Architecture Overview**:
- **MemTable**: In-memory `BTreeMap` for O(log N) writes.
- **SSTable**: Disk-based sorted files with sparse indexing and Bloom Filters.

**Compaction Strategies**:
- **Leveled Compaction**: Merges older SSTables to keep read paths efficient.

### 3. HeatMap Cache (`heatmap.rs`)

**Purpose**: Intelligent caching system achieving 95%+ hit rates

**Features**:
- **Bloom Filters**: Probabilistic data structure to quickly test if an SSTable contains a key.
- **Hot Data Tracking**: Monitors access frequency to prioritize keeping data in memory.

### 4. File Format (`format.rs`)

**Purpose**: Universal .bdb file format with integrity guarantees

**File Format Specification**:
- **Header**: Magic bytes, version, timestamp.
- **Entries**: Type, Key Len (Varint), Key, Value Len (Varint), Value, Timestamp, CRC32.
- **Footer**: Summary stats and file checksum.


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

**Rust Coding Standards**:
- Use `rustfmt` for formatting.
- Use `clippy` for linting.
- Document public APIs with rustdoc comments (`///`).

### Pull Request Process

1. **Fork and Branch**: Create feature branch from `main`
2. **Implement**: Follow coding standards and add tests
3. **Test**: Run `cargo test` locally
4. **Submit**: Create pull request with clear description

---

## ⚡ Performance Engineering

### Performance Targets

| Metric | Target | Measurement |
|--------|--------|-------------|
| **Read Throughput** | 800K+ ops/sec | `stress_test.rs` |
| **Write Throughput** | 700K+ ops/sec | `stress_test.rs` |
| **Query Latency (P99)** | <0.1ms | Real-time monitoring |

### Benchmarking

```bash
cd bindings
cargo run --release --example stress_test
```

---

## 🧪 Testing Strategy

### Unit Testing

Run unit tests for individual modules:
```bash
cargo test
```


---

<div align="center">

**[⬅️ Back to User Manual](USER_MANUAL.md)** | **[📁 File Structure](FILE_STRUCTURE.md)** | **[🔧 API Reference](API_REFERENCE.md)**

Ready to contribute? Start with the [development setup](#development-setup)!

</div>