# 🛠️ BrowserDB Developer Guide

Architecture, implementation details, and guidelines for contributors.

## 📋 Table of Contents

1. [Architecture Overview](#architecture-overview)
2. [Core Components](#core-components)
3. [Development Setup](#development-setup)
4. [Contribution Guidelines](#contribution-guidelines)
5. [Performance Engineering](#performance-engineering)
6. [Testing Strategy](#testing-strategy)
7. [Implementation Details](#implementation-details)
8. [Extending BrowserDB](#extending-browserdb)

---

## 🏗️ Architecture Overview

### System Design Philosophy

BrowserDB follows these core principles:

1. **Performance First**: Sub-millisecond queries for hot data
2. **Memory Efficient**: <50MB footprint with intelligent caching
3. **Reliability**: ACID compliance with corruption recovery
4. **Simplicity**: Clear interfaces and predictable behavior
5. **Extensibility**: Modular design for easy enhancement

### High-Level Architecture

```
┌─────────────────────────────────────────────────────────────────┐
│                        Application Layer                        │
│  ┌──────────────┐  ┌──────────────┐  ┌──────────────┐          │
│  │ JavaScript   │  │   Rust FFI   │  │ Other Langs  │          │
│  │     API      │  │   Bindings   │  │   (Future)   │          │
│  └──────────────┘  └──────────────┘  └──────────────┘          │
└─────────────────────────────┬───────────────────────────────────┘
                              │
┌─────────────────────────────┴───────────────────────────────────┐
│                       Core Engine (Zig)                        │
│  ┌──────────────┐  ┌──────────────┐  ┌──────────────┐          │
│  │  BrowserDB   │  │  HeatMap     │  │   Modes &    │          │
│  │ Coordinator  │  │   Cache      │  │  Operations  │          │
│  └──────────────┘  └──────────────┘  └──────────────┘          │
│         │                    │                    │           │
│  ┌──────┴──────────────┐     │     ┌──────────────┴──────┐    │
│  │     LSM-Tree        │     │     │    BDB Format      │    │
│  │     Storage         │     │     │    & I/O          │    │
│  └─────────────────────┘     │     └─────────────────────┘    │
│                              │                                │
│  ┌───────────────────────────┼────────────────────────────────┤
│  │                    File System                             │
│  │  ┌──────────────┐ ┌──────────────┐ ┌──────────────┐      │
│  │  │  .bdb Files  │ │  SSTables    │ │   WAL Logs   │      │
│  │  │  (Universal) │ │  (Storage)   │ │ (Recovery)   │      │
│  │  └──────────────┘ └──────────────┘ └──────────────┘      │
│  └───────────────────────────────────────────────────────────┘
└─────────────────────────────────────────────────────────────────┘
```

### Data Flow

```
Write Operation:
App → FFI → BrowserDB → LSM-Tree → MemTable → (Flush) → SSTable

Read Operation:  
App → FFI → BrowserDB → HeatMap Cache → (Miss) → LSM-Tree → SSTables

Compaction:
Background → LSM-Tree → Merge SSTables → Optimize Storage
```

---

## 🔧 Core Components

### 1. BrowserDB Coordinator (`browserdb.zig`)

**Purpose**: Central orchestrator managing all subsystems

**Key Responsibilities**:
- Database lifecycle management
- Request routing and coordination
- Mode switching orchestration
- Transaction management
- Performance monitoring

**Architecture Pattern**: Facade + Coordinator
```zig
pub const BrowserDB = struct {
    lsm_tree: *LSMTree,
    heatmap_cache: *HeatMapCache,
    file_format: *BDBFormat,
    mode_ops: *ModeOperations,
    
    pub fn put(db: *BrowserDB, key: []const u8, value: []const u8) !void {
        // 1. Validate input
        try validateKeyValue(key, value);
        
        // 2. Check cache first
        if (db.heatmap_cache.get(key)) |cached_value| {
            return db.heatmap_cache.put(key, value);
        }
        
        // 3. Store in LSM-Tree
        try db.lsm_tree.put(key, value);
        
        // 4. Update cache
        db.heatmap_cache.put(key, value);
        
        // 5. Log operation
        db.logOperation(.Write, key, value.len);
    }
};
```

**Key Design Decisions**:
- **Facade Pattern**: Simplifies complex subsystem interactions
- **Caching Strategy**: Check cache before expensive disk operations  
- **Error Propagation**: Consistent error handling across components
- **Performance Monitoring**: Built-in operation tracking

### 2. LSM-Tree Storage (`lsm_tree.zig`)

**Purpose**: High-performance storage with optimal write/read balance

**Architecture Overview**:
```
Memory Layer:          Disk Layer:
┌─────────────┐       ┌─────────────────────────────────┐
│   MemTable  │──────►│  L0 SSTables  → L1 SSTables   │
│   (32MB)    │       │  (Unsorted)    → (Sorted)      │
└─────────────┘       └─────────────────────────────────┘
      │                        │
      ▼                        ▼
  Fast Writes           Efficient Reads
```

**Compaction Strategies**:

1. **Size-Tiered Compaction**:
```
Before:    After:
┌─────┐    ┌─────┐
│ S1  │    │     │  Large SSTable created by
│ S2  │───►│ S13 │  merging similar-sized files
│ S3  │    │     │
└─────┘    └─────┘
```
- **Strategy**: Merge SSTables of similar size
- **Benefit**: Balanced read/write performance
- **Use Case**: Write-heavy workloads

2. **Tiered Compaction**:
```
Levels: L0 → L1 → L2 → L3
Size:   1×  → 10× → 100× → 1000×
```
- **Strategy**: Each level 10x larger than previous
- **Benefit**: Predictable performance
- **Use Case**: Read-heavy workloads

3. **Leveled Compaction** (Default):
```
L0: 4 SSTables max (unsorted)
L1: 10 SSTables max (sorted, 10x L0)
L2: 100 SSTables max (sorted, 10x L1)
```
- **Strategy**: Strict level size ratios
- **Benefit**: Optimal read amplification
- **Use Case**: General-purpose workloads

**Binary Search Integration**:
```zig
pub fn get(lsm: *LSMTree, key: []const u8) !?[]const u8 {
    // 1. Check MemTable first (fastest)
    if (lsm.mem_table.get(key)) |value| {
        return value;
    }
    
    // 2. Binary search across SSTables
    var best_result: ?struct { sstable: *SSTable, offset: u64 } = null;
    
    for (lsm.sstable_levels) |level| {
        for (level.files) |*file| {
            if (try binarySearchSSTable(file, key)) |result| {
                // Keep the most recent (higher level)
                if (best_result == null or result.level > best_result.?.level) {
                    best_result = result;
                }
            }
        }
    }
    
    // 3. Return most recent match
    if (best_result) |result| {
        return try result.sstable.readValue(result.offset);
    }
    
    return null;
}
```

### 3. HeatMap Cache (`heatmap_indexing.zig`)

**Purpose**: Intelligent caching system achieving 95%+ hit rates

**Algorithm Overview**:
```
┌─────────────────────────────────────────────────────────────┐
│                    HeatMap Heat Analysis                    │
│                                                             │
│  ┌──────────┐  ┌──────────┐  ┌──────────┐  ┌──────────┐   │
│  │   HOT    │  │   WARM   │  │   COLD   │  │  FROZEN  │   │
│  │  95%     │  │   4%     │  │   0.9%   │  │  0.1%    │   │
│  │  hits    │  │  hits    │  │  hits    │  │  hits    │   │
│  └──────────┘  └──────────┘  └──────────┘  └──────────┘   │
│      ↓            ↓            ↓            ↓             │
│  Keep in      Keep in      Evict when     Immediate       │
│  cache       cache         needed        eviction        │
└─────────────────────────────────────────────────────────────┘
```

**Implementation Strategy**:
```zig
pub const HeatMapCache = struct {
    // Access tracking
    access_count: std.AutoHashMap(u64, AccessInfo),
    last_access: std.AutoHashMap(u64, u64),
    
    // Frequency analysis
    total_accesses: u64,
    frequency_bins: [256]u64, // 256 frequency bins
    
    // Memory management
    memory_pool: MemoryPool,
    eviction_queue: PriorityQueue(CacheEntry, compareByColdness),
    
    pub fn get(c: *HeatMapCache, key: []const u8) !?[]const u8 {
        const key_hash = hashKey(key);
        
        // Track access
        const current_time = getTimestamp();
        const access_count = c.access_count.get(key_hash) orelse 0;
        c.access_count.put(key_hash, access_count + 1);
        c.last_access.put(key_hash, current_time);
        c.total_accesses += 1;
        
        // Update frequency distribution
        const bin_index = @min(access_count, 255);
        c.frequency_bins[bin_index] += 1;
        
        // Perform lookup
        if (c.memory_pool.get(key)) |value| {
            // Reinsert to update priority
            c.eviction_queue.update(key, value);
            return value;
        }
        
        return null;
    }
};
```

**Key Features**:
- **Adaptive Sizing**: Automatically adjusts to workload patterns
- **Frequency Tracking**: Real-time access pattern analysis
- **Memory Efficiency**: Constant memory usage regardless of data size
- **Performance Metrics**: Built-in cache analytics

### 4. File Format (`bdb_format.zig`)

**Purpose**: Universal .bdb file format with integrity guarantees

**File Format Specification**:
```
┌─────────────────────────────────────────────────────────────┐
│                        BDB File Header                      │
│  ┌─────────────────────────────────────────────────────────┐ │
│  │ Magic Number: "BROWSERDB" (8 bytes)                   │ │
│  │ Version: 1 (4 bytes)                                   │ │
│  │ Flags: Compression, Encryption flags (4 bytes)         │ │
│  │ File Size: Total file size (8 bytes)                   │ │
│  │ Entry Count: Number of entries (8 bytes)               │ │
│  │ Created Time: Unix timestamp (8 bytes)                 │ │
│  │ Header CRC32: Checksum (4 bytes)                       │ │
│  │ Reserved: 16 bytes padding                             │ │
│  └─────────────────────────────────────────────────────────┘ │
├─────────────────────────────────────────────────────────────┤
│                        Entry Stream                         │
│  ┌────────────┬────────────┬────────────┬──────────────────┐ │
│  │Key Length  │    Key     │ Value      │    Entry CRC32   │ │
│  │  (2 bytes) │  (Variable)│ Length     │    (4 bytes)     │ │
│  │            │            │(4 bytes)   │                  │ │
│  └────────────┴────────────┴────────────┴──────────────────┘ │
│  │ ┌─────────────────────────────────────────────────────┐  │ │
│  │ │               Value Data                            │  │ │
│  │ │               (Variable Length)                     │  │ │
│  │ └─────────────────────────────────────────────────────┘  │ │
│  │                                                             │
│  │  [Repeat for all entries]                                 │
├─────────────────────────────────────────────────────────────┤
│                        File Footer                           │
│  ┌─────────────────────────────────────────────────────────┐ │
│  │ Metadata Hash: SHA-256 of header + entries (32 bytes)   │ │
│  │ Data Size: Total size of entry data (8 bytes)          │ │
│  │ File CRC32: Complete file checksum (4 bytes)           │ │
│  │ Index Offset: Offset to entry index (8 bytes)          │ │
│  │ Reserved: 12 bytes padding                              │ │
│  └─────────────────────────────────────────────────────────┘ │
└─────────────────────────────────────────────────────────────┘
```

**Compression Support**:
```zig
pub const CompressionType = enum {
    None,
    LZ4,       // Fast, moderate compression
    Zlib,      // Balanced compression/speed
    Zstandard, // High compression, medium speed
};

pub fn compress(data: []const u8, compression: CompressionType) ![]const u8 {
    return switch (compression) {
        .None => data,
        .LZ4 => try compressLZ4(data),
        .Zlib => try compressZlib(data),
        .Zstandard => try compressZstandard(data),
    };
}

// Streaming compression for large data
pub fn compressStream(reader: anytype, writer: anytype, compression: CompressionType) !void {
    var compressor = try createCompressor(compression);
    defer compressor.deinit();
    
    var buffer: [8192]u8 = undefined;
    while (try reader.read(&buffer)) > 0 {
        const compressed = try compressor.compress(buffer[0..bytes_read]);
        try writer.writeAll(compressed);
    }
    
    try compressor.flush();
}
```

**Integrity Validation**:
```zig
pub const ValidationResult = struct {
    is_valid: bool,
    errors: []ValidationError,
    warnings: []ValidationWarning,
    
    pub fn validateFile(file: *BDBFile) !ValidationResult {
        var result = ValidationResult{
            .is_valid = true,
            .errors = &.{},
            .warnings = &.{},
        };
        
        // 1. Header validation
        try validateHeader(file, &result);
        
        // 2. Entry stream validation
        try validateEntries(file, &result);
        
        // 3. Footer validation
        try validateFooter(file, &result);
        
        // 4. CRC32 verification
        try verifyChecksums(file, &result);
        
        return result;
    }
};
```

### 5. Mode Operations (`modes_operations.zig`)

**Purpose**: Atomic mode switching between Persistent and Ultra modes

**Mode Characteristics**:

| Aspect | Persistent Mode | Ultra Mode |
|--------|----------------|------------|
| **Storage Location** | Disk + RAM Cache | RAM Only |
| **Persistence** | Survives restart | Volatile |
| **Access Speed** | Fast (cached) | Instant |
| **Memory Usage** | <50MB | Unlimited* |
| **Use Case** | User data, settings | Cache, sessions |
| **Durability** | Full ACID | Eventual consistency |

**Mode Switching Implementation**:
```zig
pub fn switchToPersistent(db: *BrowserDB) !ModeSwitchResult {
    const start_time = getTimestamp();
    
    // 1. Validate current state
    try validatePersistentModeRequirements(db);
    
    // 2. Create transition snapshot
    const snapshot = try createMemorySnapshot(db.ultra_mode_data);
    
    // 3. Migrate data to disk
    const migration_result = try migrateToPersistent(&snapshot);
    
    // 4. Validate migration
    try validateMigrationIntegrity(&snapshot, migration_result);
    
    // 5. Atomic switch
    db.setMode(.Persistent);
    db.persistent_mode = migration_result;
    
    // 6. Cleanup
    snapshot.deinit();
    
    const duration = getTimestamp() - start_time;
    return ModeSwitchResult{
        .success = true,
        .duration = duration,
        .bytes_migrated = migration_result.bytes_written,
    };
}
```

**Rollback Capability**:
```zig
pub fn rollbackModeSwitch(db: *BrowserDB, original_mode: DatabaseMode) !void {
    // Restore original mode state
    switch (original_mode) {
        .Ultra => {
            db.setMode(.Ultra);
            // Restore Ultra mode data from backup
            if (db.ultra_backup) |backup| {
                try restoreUltraModeData(backup);
                db.ultra_backup = null;
            }
        },
        .Persistent => {
            db.setMode(.Persistent);
            // Restore Persistent mode state
            if (db.persistent_backup) |backup| {
                try restorePersistentModeState(backup);
                db.persistent_backup = null;
            }
        },
    }
    
    // Invalidate caches to prevent corruption
    db.heatmap_cache.invalidate();
}
```

---

## 🛠️ Development Setup

### Prerequisites

```bash
# Required versions
Zig:    >= 0.13.0
Rust:   >= 1.75.0  
CMake:  >= 3.16.0
Git:    >= 2.30.0

# Optional for development
valgrind  # Memory debugging
perf      # Performance profiling
llvm      # Static analysis
```

### Build System

**Core Engine (Zig)**:
```bash
cd core

# Debug build
zig build

# Release build
zig build -Drelease-safe

# Release with optimizations
zig build -Drelease-fast

# Run tests
zig build test

# Run specific test
zig build test --test-file lsm_tree_tests.zig
```

**Rust Bindings**:
```bash
cd bindings

# Debug build
cargo build

# Release build
cargo build --release

# Run tests
cargo test

# Run integration tests
cargo test --test integration_tests

# Generate documentation
cargo doc --no-deps
```

### Development Environment

**Recommended VSCode Setup**:
```json
{
    "recommendations": [
        "ziglang.vscode-zig",
        "rust-lang.rust-analyzer",
        "ms-vscode.cpptools"
    ],
    "settings": {
        "zig.buildOnSave": "package",
        "rust-analyzer.cargo.features": ["all"],
        "files.associations": {
            "*.zig": "zig"
        }
    }
}
```

**Debugging Configuration** (`.vscode/launch.json`):
```json
{
    "version": "0.2.0",
    "configurations": [
        {
            "name": "Debug BrowserDB Core",
            "type": "cppdbg",
            "request": "launch",
            "program": "${workspaceFolder}/core/zig-cache/bin/browserdb_test",
            "args": [],
            "cwd": "${workspaceFolder}/core",
            "environment": [],
            "preLaunchTask": "build-debug"
        }
    ]
}
```

---

## 🤝 Contribution Guidelines

### Code Style

**Zig Coding Standards**:
```zig
// ✅ Good: Clear function names, proper error handling
pub fn getUserProfile(db: *BrowserDB, user_id: []const u8) !?UserProfile {
    const key = try std.fmt.allocPrint(allocator, "user:{}:profile", .{user_id});
    defer allocator.free(key);
    
    if (try db.get(key)) |data| {
        return try parseUserProfile(data, allocator);
    }
    
    return null;
}

// ❌ Bad: Poor naming, no error handling
pub fn get(id: []u8) ?Profile {
    var k = "user:" ++ id ++ ":p";
    if (db.get(k)) |d| return parse(d);
    return null;
}
```

**Rust Coding Standards**:
```rust
// ✅ Good: Proper error handling, clear documentation
/// Retrieves a user profile from the database.
/// 
/// # Arguments
/// * `db` - The database connection
/// * `user_id` - The unique user identifier
/// 
/// # Returns
/// * `Ok(Some(Profile))` - If user exists
/// * `Ok(None)` - If user not found
/// * `Err(DBError)` - If database error occurs
/// 
/// # Examples
/// ```
/// let profile = get_user_profile(&db, "user123")?;
/// match profile {
///     Some(p) => println!("Found user: {}", p.name),
///     None => println!("User not found"),
/// }
/// ```
pub fn get_user_profile(db: &BrowserDB, user_id: &str) -> Result<Option<UserProfile>, DBError> {
    let key = format!("user:{}:profile", user_id);
    
    match db.get(key.as_bytes())? {
        Some(data) => {
            let profile = serde_json::from_slice(&data)
                .map_err(DBError::InvalidArgument)?;
            Ok(Some(profile))
        },
        None => Ok(None),
    }
}
```

### Testing Requirements

**Unit Test Structure**:
```zig
// core/tests/lsm_tree_tests.zig
const testing = @import("std").testing;

test "LSMTree basic put and get" {
    const allocator = testing.allocator;
    var lsm = try LSMTree.init(allocator, .{});
    defer lsm.deinit();
    
    // Test data
    const key = "test_key";
    const value = "test_value";
    
    // Write operation
    try lsm.put(key, value);
    
    // Read operation
    const retrieved = lsm.get(key);
    try testing.expectEqualStrings(value, retrieved.?);
}

test "LSMTree compaction" {
    // Create enough data to trigger compaction
    var lsm = try LSMTree.init(testing.allocator, .{});
    defer lsm.deinit();
    
    // Fill MemTable to trigger flush
    for (0..10000) |i| {
        const key = std.fmt.allocPrintZ(testing.allocator, "key_{}", .{i}) catch unreachable;
        defer testing.allocator.free(key);
        try lsm.put(key, "value");
    }
    
    // Verify compaction occurred
    try testing.expect(lsm.sstable_levels.len > 1);
}
```

**Integration Test Structure**:
```rust
// bindings/tests/integration_tests.rs
use browserdb::*;

#[tokio::test]
async fn test_database_lifecycle() {
    let temp_dir = tempfile::tempdir().unwrap();
    let db_path = temp_dir.path().join("test.bdb");
    
    // Create database
    let db = BrowserDB::create(db_path).unwrap();
    
    // Insert test data
    db.put("key1", "value1").unwrap();
    db.put("key2", "value2").unwrap();
    
    // Verify data
    assert_eq!(db.get("key1").unwrap(), Some("value1".as_bytes()));
    assert_eq!(db.get("key2").unwrap(), Some("value2".as_bytes()));
    
    // Close and reopen
    drop(db);
    let db = BrowserDB::open(db_path).unwrap();
    
    // Verify persistence
    assert_eq!(db.get("key1").unwrap(), Some("value1".as_bytes()));
}

#[tokio::test]
async fn test_performance_benchmarks() {
    let temp_dir = tempfile::tempdir().unwrap();
    let db_path = temp_dir.path().join("perf_test.bdb");
    
    let db = BrowserDB::open_or_create(db_path).unwrap();
    
    // Benchmark write performance
    let start = std::time::Instant::now();
    for i in 0..10000 {
        let key = format!("key_{}", i);
        let value = format!("value_{}", i);
        db.put(&key, &value).unwrap();
    }
    let write_duration = start.elapsed();
    
    // Benchmark read performance
    let start = std::time::Instant::now();
    for i in 0..10000 {
        let key = format!("key_{}", i);
        let _ = db.get(&key).unwrap();
    }
    let read_duration = start.elapsed();
    
    // Assert performance requirements
    assert!(write_duration.as_millis() < 1000); // < 1 second for 10k writes
    assert!(read_duration.as_millis() < 100);   // < 100ms for 10k reads
}
```

### Pull Request Process

1. **Fork and Branch**: Create feature branch from `main`
2. **Implement**: Follow coding standards and add tests
3. **Test**: Run full test suite locally
4. **Document**: Update documentation if needed
5. **Submit**: Create pull request with clear description

**PR Template**:
```markdown
## Description
Brief description of changes

## Type of Change
- [ ] Bug fix
- [ ] New feature  
- [ ] Performance improvement
- [ ] Documentation update
- [ ] Refactoring

## Testing
- [ ] Unit tests pass
- [ ] Integration tests pass
- [ ] Performance benchmarks maintained
- [ ] Added tests for new functionality

## Checklist
- [ ] Code follows style guidelines
- [ ] Self-review completed
- [ ] Documentation updated
- [ ] Tests added/updated
```

---

## ⚡ Performance Engineering

### Performance Targets

| Metric | Target | Measurement |
|--------|--------|-------------|
| **Read Throughput** | 150K+ ops/sec | `perf_benchmarks.zig` |
| **Write Throughput** | 12K+ ops/sec | `perf_benchmarks.zig` |
| **Query Latency (P99)** | <1ms | Real-time monitoring |
| **Cache Hit Rate** | 95%+ | HeatMap analytics |
| **Memory Usage** | <50MB | Memory profiling |
| **Startup Time** | <100ms | Application metrics |

### Profiling Tools

**Zig Performance Profiling**:
```bash
# Build with profiling support
zig build -Drelease-safe -Dprofile

# Run with perf
perf record -g ./zig-cache/bin/browserdb_benchmark
perf report

# Memory profiling
valgrind --tool=massif ./zig-cache/bin/browserdb_test
ms_print massif.out.*
```

**Rust Performance Profiling**:
```bash
# Add to Cargo.toml
[dependencies]
criterion = "0.5"

# Benchmark tests
cargo bench

# CPU profiling
cargo install cargo-flamegraph
cargo flamegraph --bin browserdb_benchmark
```

### Performance Optimization Techniques

**1. Memory Pool Allocation**:
```zig
// Avoid frequent allocations
pub const MemoryPool = struct {
    blocks: std.ArrayList(MemoryBlock),
    
    pub fn alloc(pool: *MemoryPool, size: usize) ![]u8 {
        // Reuse existing blocks when possible
        for (pool.blocks.items, 0..) |*block, i| {
            if (block.size >= size and !block.in_use) {
                block.in_use = true;
                return block.data[0..size];
            }
        }
        
        // Allocate new block
        const new_block = try pool.blocks.addOne();
        new_block.* = MemoryBlock{
            .data = try pool.allocator.alloc(u8, size),
            .size = size,
            .in_use = true,
        };
        
        return new_block.data;
    }
};
```

**2. SIMD Optimizations**:
```zig
// Bulk operations using SIMD
pub fn bulkHash(keys: []const []const u8) ![256]u64 {
    var hashes: [256]u64 = undefined;
    @memset(@as([*]u8, @ptrCast(&hashes)), 0, @sizeOf(@TypeOf(hashes)));
    
    // Process 32 keys at a time using SIMD
    var i: usize = 0;
    while (i + 32 <= keys.len) : (i += 32) {
        const batch = keys[i..i+32];
        const simd_hashes = @simd(u8, 32).hashBatch(batch);
        
        for (simd_hashes, 0..) |hash, j| {
            const key_idx = i + j;
            hashes[key_idx % 256] +%= hash;
        }
    }
    
    return hashes;
}
```

**3. Cache-Friendly Data Structures**:
```zig
// Cache line alignment for better performance
pub const CacheLine = struct {
    const align_size = 64; // Typical CPU cache line size
    
    data: [align_size]u8 align(align_size),
    
    pub fn init() CacheLine {
        return CacheLine{ .data = undefined };
    }
};

// Hot data structures in separate cache lines
pub const HotData = struct {
    hot_access_count: u64 align(64),
    hot_last_access: u64 align(64),
    hot_metadata: HotMetadata align(64),
    
    cold_data: ColdData, // Can be in different cache line
};
```

### Benchmarking Strategy

**Microbenchmarks**:
```zig
// core/tests/perf_benchmarks.zig
const perf = @import("../perf.zig");

test "write throughput benchmark" {
    const allocator = testing.allocator;
    var db = try BrowserDB.init(allocator, "benchmark.bdb");
    defer db.deinit();
    
    const iterations = 100000;
    const timer = perf.Timer.start();
    
    for (0..iterations) |i| {
        const key = std.fmt.allocPrintZ(allocator, "key_{:06}", .{i}) catch unreachable;
        defer allocator.free(key);
        try db.put(key, "value");
    }
    
    const duration = timer.elapsed();
    const throughput = @as(f64, iterations) / (duration.asSeconds());
    
    try testing.expect(throughput >= 12000); // 12K+ ops/sec
    std.debug.print("Write throughput: {:.0} ops/sec\n", .{throughput});
}
```

**Macro Benchmarks**:
```rust
// Real-world workload simulation
async fn simulate_browser_workload(db: &BrowserDB) {
    // Simulate typical browser operations
    
    // 1. History queries (80% reads, 20% writes)
    for _ in 0..10000 {
        if rand::random::<f32>() < 0.8 {
            // Read operation
            let key = format!("history:{:020}", rand::random::<u64>());
            let _ = db.get(&key);
        } else {
            // Write operation
            let key = format!("history:{:020}", get_timestamp());
            let value = generate_url_data();
            db.put(&key, &value).unwrap();
        }
    }
    
    // 2. Bookmark operations (50/50 read/write)
    for _ in 0..5000 {
        if rand::random::<f32>() < 0.5 {
            let key = format!("bookmark:{}", rand::random::<u32>());
            let _ = db.get(&key);
        } else {
            let key = format!("bookmark:{}", get_timestamp());
            let value = generate_bookmark_data();
            db.put(&key, &value).unwrap();
        }
    }
    
    // 3. Settings reads (95% reads)
    for _ in 0..2000 {
        let key = format!("settings:theme");
        let _ = db.get(&key);
    }
}
```

---

## 🧪 Testing Strategy

### Test Architecture

```
┌─────────────────────────────────────────────────────────────┐
│                        Test Pyramid                         │
│                                                             │
│        ┌─────────────────────────────────────┐             │
│        │      End-to-End Integration         │             │
│        │        (Real Browser Tests)         │             │
│        │                                     │             │
│        │   ┌─────────────────────────────┐   │             │
│        │   │    Component Integration     │   │             │
│        │   │      (Multi-Module Tests)   │   │             │
│        │   │                             │   │             │
│        │   │   ┌─────────────────────┐   │   │             │
│        │   │   │    Unit Tests       │   │   │             │
│        │   │   │  (Individual Units) │   │   │             │
│        │   │   └─────────────────────┘   │   │             │
│        │   └─────────────────────────────┘   │             │
│        └─────────────────────────────────────┘             │
│                                                             │
│  Coverage Goals:                                           │
│  • Unit Tests:     95%+                                    │
│  • Integration:    85%+                                    │
│  • End-to-End:     70%+                                    │
└─────────────────────────────────────────────────────────────┘
```

### Unit Testing

**Component Coverage**:

1. **LSM-Tree Tests** (`lsm_tree_tests.zig`):
```zig
test "memtable flush on size limit" {
    var lsm = try LSMTree.init(testing.allocator, .{ .memtable_size = 1024 });
    defer lsm.deinit();
    
    // Fill memtable beyond limit
    var total_size: usize = 0;
    for (0..100) |i| {
        const key = std.fmt.allocPrintZ(testing.allocator, "key_{:03}", .{i}) catch unreachable;
        defer testing.allocator.free(key);
        const value = std.fmt.allocPrintZ(testing.allocator, "value_{:010}", .{i}) catch unreachable;
        defer testing.allocator.free(value);
        
        total_size += key.len + value.len;
        try lsm.put(key, value);
    }
    
    // Verify memtable was flushed
    try testing.expect(lsm.mem_table.isEmpty());
    try testing.expect(lsm.sstable_levels[0].files.len > 0);
}

test "binary search across multiple sstables" {
    var lsm = try LSMTree.init(testing.allocator, .{ .memtable_size = 64 });
    defer lsm.deinit();
    
    // Create multiple SSTables with overlapping ranges
    try populateSSTables(&lsm, "a", "m"); // First SSTable
    try populateSSTables(&lsm, "n", "z"); // Second SSTable
    
    // Test binary search finds correct SSTable
    const result = lsm.binarySearch("hello");
    try testing.expect(result.sstable != null);
    try testing.expectEqualStrings("hello", result.key.?);
}
```

2. **HeatMap Cache Tests** (`heatmap_indexing_tests.zig`):
```zig
test "cache hit rate calculation" {
    var cache = try HeatMapCache.init(testing.allocator, .{ .max_size = 1024 * 1024 });
    defer cache.deinit();
    
    // Perform known access pattern
    const keys = [_][]const u8{ "hot", "warm", "cold" };
    
    // Access "hot" 100 times
    for (0..100) |_| {
        _ = cache.get("hot").?;
        cache.put("hot", "hot_value").?;
    }
    
    // Access "warm" 10 times
    for (0..10) |_| {
        _ = cache.get("warm").?;
        cache.put("warm", "warm_value").?;
    }
    
    // Access "cold" once
    _ = cache.get("cold").?;
    cache.put("cold", "cold_value").?;
    
    // Verify hit rate calculation
    const stats = cache.getStatistics();
    try testing.expect(stats.hit_rate > 0.95); // 95%+ hit rate
}
```

### Integration Testing

**Cross-Component Tests**:
```rust
// bindings/tests/integration_tests.rs

#[tokio::test]
async fn test_lsm_tree_with_cache_integration() {
    let temp_dir = tempfile::tempdir().unwrap();
    let db_path = temp_dir.path().join("integration_test.bdb");
    
    let db = BrowserDB::open(db_path).unwrap();
    
    // Test cache-LSM integration
    let start = std::time::Instant::now();
    
    // Write data
    for i in 0..1000 {
        let key = format!("key_{}", i);
        let value = format!("value_{}", i);
        db.put(&key, &value).unwrap();
    }
    
    // Read data (should hit cache after first read)
    for i in 0..1000 {
        let key = format!("key_{}", i);
        let result = db.get(&key).unwrap();
        assert_eq!(result, Some(format!("value_{}", i).into_bytes()));
    }
    
    let duration = start.elapsed();
    
    // Second read should be much faster (cache hits)
    let cache_start = std::time::Instant::now();
    for i in 0..1000 {
        let key = format!("key_{}", i);
        let _ = db.get(&key).unwrap();
    }
    let cache_duration = cache_start.elapsed();
    
    // Cache should be significantly faster
    assert!(cache_duration < duration / 10);
}

#[tokio::test] 
async fn test_mode_switching_integration() {
    let temp_dir = tempfile::tempdir().unwrap();
    let db_path = temp_dir.path().join("mode_test.bdb");
    
    let mut db = BrowserDB::open_ultra(db_path).unwrap();
    
    // Insert data in Ultra mode
    db.put("test:key", "ultra_value").unwrap();
    
    // Switch to Persistent mode
    db.switch_to_persistent().unwrap();
    
    // Verify data persists
    assert_eq!(db.get("test:key").unwrap(), Some(b"ultra_value".to_vec()));
    
    // Close and reopen (simulating app restart)
    drop(db);
    let mut db = BrowserDB::open(db_path).unwrap();
    
    // Data should still be available
    assert_eq!(db.get("test:key").unwrap(), Some(b"ultra_value".to_vec()));
}
```

### End-to-End Testing

**Real Browser Integration**:
```html
<!-- tests/e2e/browser_integration.html -->
<!DOCTYPE html>
<html>
<head>
    <title>BrowserDB E2E Test</title>
</head>
<body>
    <div id="results"></div>
    <script src="browserdb.js"></script>
    <script>
        async function runE2ETests() {
            const results = document.getElementById('results');
            
            try {
                // Test 1: Basic operations
                const db = await BrowserDB.open('e2e_test.bdb');
                await db.put('test:key', 'test_value');
                const value = await db.get('test:key');
                results.innerHTML += `<p>✅ Basic operations: ${value === 'test_value' ? 'PASS' : 'FAIL'}</p>`;
                
                // Test 2: Performance
                const perf_start = performance.now();
                for (let i = 0; i < 10000; i++) {
                    await db.put(`perf:key:${i}`, `perf:value:${i}`);
                }
                const perf_end = performance.now();
                const write_time = perf_end - perf_start;
                results.innerHTML += `<p>✅ Write performance: ${write_time < 1000 ? 'PASS' : 'FAIL'} (${write_time}ms)</p>`;
                
                // Test 3: Persistence
                await db.close();
                const db2 = await BrowserDB.open('e2e_test.bdb');
                const persisted = await db2.get('test:key');
                results.innerHTML += `<p>✅ Persistence: ${persisted === 'test_value' ? 'PASS' : 'FAIL'}</p>`;
                
            } catch (error) {
                results.innerHTML += `<p>❌ Error: ${error.message}</p>`;
            }
        }
        
        runE2ETests();
    </script>
</body>
</html>
```

### Performance Testing

**Automated Performance Regression**:
```zig
// core/tests/performance_regression.zig
const RegressionTest = struct {
    baseline_performance: PerformanceMetrics,
    current_performance: PerformanceMetrics,
    tolerance: f64, // 10% tolerance
    
    pub fn runRegressionTest() !bool {
        const baseline = try loadBaselinePerformance("perf_baseline.json");
        const current = try runCurrentPerformanceTests();
        
        // Check read performance regression
        const read_regression = (baseline.read_throughput - current.read_throughput) / baseline.read_throughput;
        if (read_regression > 0.10) {
            std.debug.print("❌ Read performance regression: {:.1}%\n", .{read_regression * 100});
            return false;
        }
        
        // Check write performance regression
        const write_regression = (baseline.write_throughput - current.write_throughput) / baseline.write_throughput;
        if (write_regression > 0.10) {
            std.debug.print("❌ Write performance regression: {:.1}%\n", .{write_regression * 100});
            return false;
        }
        
        std.debug.print("✅ No performance regressions detected\n");
        return true;
    }
};
```

---

## 🔬 Implementation Details

### Memory Management

**Arena Allocator Pattern**:
```zig
pub const ArenaAllocator = struct {
    memory: []u8,
    current_offset: usize,
    initial_offset: usize,
    allocator: std.mem.Allocator,
    
    pub fn init(allocator: std.mem.Allocator, size: usize) !ArenaAllocator {
        return ArenaAllocator{
            .memory = try allocator.alloc(u8, size),
            .current_offset = 0,
            .initial_offset = 0,
            .allocator = allocator,
        };
    }
    
    pub fn alloc(arena: *ArenaAllocator, size: usize, alignment: u29) ![]u8 {
        const aligned_offset = std.mem.alignForward(arena.current_offset, alignment);
        
        if (aligned_offset + size > arena.memory.len) {
            return error.OutOfMemory;
        }
        
        const slice = arena.memory[aligned_offset..aligned_offset + size];
        arena.current_offset = aligned_offset + size;
        return slice;
    }
    
    pub fn reset(arena: *ArenaAllocator) void {
        arena.current_offset = arena.initial_offset;
    }
    
    pub fn deinit(arena: *ArenaAllocator) void {
        arena.allocator.free(arena.memory);
    }
};
```

**Object Pool for Reduced Allocation**:
```zig
pub const ObjectPool = struct {
    free_objects: std.ArrayList(*anyopaque),
    create_fn: fn () *anyopaque,
    destroy_fn: fn (*anyopaque) void,
    max_size: usize,
    
    pub fn get(pool: *ObjectPool) !*anyopaque {
        if (pool.free_objects.pop()) |obj| {
            return obj;
        }
        
        if (pool.free_objects.items.len + 1 > pool.max_size) {
            return error.PoolExhausted;
        }
        
        return pool.create_fn();
    }
    
    pub fn release(pool: *ObjectPool, obj: *anyopaque) void {
        if (pool.free_objects.items.len < pool.max_size) {
            pool.free_objects.append(obj) catch {};
        } else {
            pool.destroy_fn(obj);
        }
    }
};
```

### Concurrency Patterns

**Lock-Free Operations**:
```zig
// Atomic operations for single-writer, multiple-reader scenarios
pub const AtomicSSTableRef = struct {
    generation: std.atomic.Atomic(u64),
    sstable: *SSTable,
    
    pub fn readRef(atomic_ref: *AtomicSSTableRef) *SSTable {
        var generation = atomic_ref.generation.load(.Acquire);
        const sstable = atomic_ref.sstable;
        
        // Re-read generation to ensure consistency
        while (atomic_ref.generation.load(.Acquire) != generation) {
            generation = atomic_ref.generation.load(.Acquire);
        }
        
        return sstable;
    }
    
    pub fn writeRef(atomic_ref: *AtomicSSTableRef, new_sstable: *SSTable) void {
        atomic_ref.sstable = new_sstable;
        atomic_ref.generation.fetchAdd(1, .Release);
    }
};
```

**Work Queue for Background Tasks**:
```zig
pub const WorkQueue = struct {
    tasks: std.ArrayList(BackgroundTask),
    workers: []std.Thread,
    task_channel: std.atomic.Queue(BackgroundTask),
    shutdown: std.atomic.Atomic(bool),
    
    pub fn enqueue(queue: *WorkQueue, task: BackgroundTask) !void {
        const task_node = try queue.task_channel.enqueue(task);
        
        // Wake up worker if available
        if (queue.workers.len > 0) {
            queue.workers[0].wake(); // Wake first available worker
        }
    }
    
    pub fn workerLoop(queue: *WorkQueue, worker_id: usize) void {
        while (!queue.shutdown.load(.Acquire)) {
            if (queue.task_channel.dequeue()) |task| {
                executeTask(task);
            } else {
                // Wait for new tasks
                std.Thread.yield();
            }
        }
    }
};
```

### Error Handling Strategy

**Comprehensive Error Types**:
```zig
pub const DBError = error{
    // Not found errors
    KeyNotFound,
    FileNotFound,
    
    // Corruption and integrity
    DataCorruption,
    ChecksumMismatch,
    InvalidFileFormat,
    
    // I/O and system errors
    IOError,
    DiskFull,
    PermissionDenied,
    OutOfMemory,
    
    // Validation errors
    InvalidArgument,
    ValueTooLarge,
    InvalidKey,
    
    // Concurrency errors
    LockTimeout,
    TransactionConflict,
    
    // Mode operation errors
    ModeSwitchFailed,
    MigrationFailed,
} || std.os.WriteError;
```

**Error Recovery Mechanisms**:
```zig
pub const RecoveryManager = struct {
    recovery_strategies: std.AutoHashMap(DBError, RecoveryStrategy),
    backup_manager: *BackupManager,
    
    pub fn attemptRecovery(manager: *RecoveryManager, error: DBError) !bool {
        if (manager.recovery_strategies.get(error)) |strategy| {
            return switch (strategy) {
                .Retry => manager.attemptRetry(error),
                .RestoreFromBackup => manager.restoreFromBackup(),
                .RepairCorruption => manager.repairCorruption(),
                .ReopenFile => manager.reopenFile(),
                .RestartMode => manager.restartMode(),
            };
        }
        
        return false; // No recovery strategy available
    }
    
    fn repairCorruption(manager: *RecoveryManager) !bool {
        // 1. Create backup of current state
        const backup = try manager.backup_manager.createEmergencyBackup();
        
        // 2. Attempt file repair
        const repair_result = try performFileRepair();
        
        // 3. Validate repair
        if (try validateRepairedFile()) {
            std.debug.print("✅ Corruption repair successful\n");
            return true;
        } else {
            // 4. Restore from backup if repair failed
            try manager.backup_manager.restoreFromBackup(backup);
            std.debug.print("⚠️ Repair failed, restored from backup\n");
            return false;
        }
    }
};
```

---

## 🚀 Extending BrowserDB

### Adding New Compression Algorithms

**Interface Definition**:
```zig
pub const CompressionAlgorithm = struct {
    name: []const u8,
    compress_fn: fn ([]const u8) ![]const u8,
    decompress_fn: fn ([]const u8) ![]const u8,
    getCompressionRatio: fn ([]const u8) f64,
    
    pub fn register(algorithm: CompressionAlgorithm) !void {
        // Add to global compression registry
        try compression_registry.add(algorithm);
    }
};

// Example: Custom LZ77 implementation
pub const LZ77Compression = struct {
    pub const algorithm = CompressionAlgorithm{
        .name = "LZ77-Custom",
        .compress_fn = compressLZ77Custom,
        .decompress_fn = decompressLZ77Custom,
        .getCompressionRatio = calculateLZ77Ratio,
    };
    
    fn compressLZ77Custom(data: []const u8) ![]const u8 {
        var output = std.ArrayList(u8).init(std.heap.page_allocator);
        defer output.deinit();
        
        // Custom LZ77 implementation
        var i: usize = 0;
        while (i < data.len) {
            const match = findLongestMatch(data, i);
            if (match.length >= 3) {
                // Output (length, distance) pair
                try output.append(@as(u8, match.length));
                try output.append(@as(u8, match.distance));
                i += match.length;
            } else {
                // Output literal byte
                try output.append(data[i]);
                i += 1;
            }
        }
        
        return output.toOwnedSlice();
    }
};
```

### Custom Indexing Strategies

**Building Alternative Indexes**:
```zig
pub const BTreeIndex = struct {
    root_node: *BTreeNode,
    key_type: type,
    value_type: type,
    order: usize, // B-tree order
    
    pub fn init(key_type: type, value_type: type, order: usize) BTreeIndex {
        return BTreeIndex{
            .root_node = null,
            .key_type = key_type,
            .value_type = value_type,
            .order = order,
        };
    }
    
    pub fn insert(index: *BTreeIndex, key: anytype, value: anytype) !void {
        if (index.root_node == null) {
            index.root_node = try BTreeNode.create(index.order, key_type, value_type);
        }
        
        if (index.root_node.isFull()) {
            // Split root node
            const new_root = try BTreeNode.create(index.order, key_type, value_type);
            new_root.children[0] = index.root_node;
            try new_root.splitChild(0);
            index.root_node = new_root;
        }
        
        try index.root_node.insertNonFull(key, value);
    }
    
    pub fn search(index: *BTreeIndex, key: anytype) !?anytype {
        var node = index.root_node;
        
        while (node != null) {
            var i: usize = 0;
            
            // Find first key >= search key
            while (i < node.key_count and node.keys[i] < key) {
                i += 1;
            }
            
            if (i < node.key_count and node.keys[i] == key) {
                return node.values[i];
            } else if (node.isLeaf()) {
                return null;
            } else {
                node = node.children[i];
            }
        }
        
        return null;
    }
};
```

### Plugin Architecture

**Plugin System Design**:
```zig
pub const Plugin = struct {
    name: []const u8,
    version: [3]u32, // Major, minor, patch
    init_fn: fn (*PluginContext) !void,
    destroy_fn: fn (*PluginContext) !void,
    
    // Optional plugin functions
    on_insert: ?fn (*PluginContext, []const u8, []const u8) !void,
    on_query: ?fn (*PluginContext, []const u8) ![]const u8,
    on_compact: ?fn (*PluginContext, *CompactionContext) !void,
};

pub const PluginManager = struct {
    plugins: std.ArrayList(*Plugin),
    context: PluginContext,
    
    pub fn loadPlugin(manager: *PluginManager, plugin_path: []const u8) !void {
        // Load shared library
        const plugin_lib = try std.dl.open(plugin_path);
        defer plugin_lib.close();
        
        // Get plugin interface
        const get_plugin_fn = plugin_lib.symbol("getPlugin") orelse 
            return error.InvalidPlugin;
        const plugin_ptr = @as(*Plugin, @ptrCast(get_plugin_fn));
        
        // Initialize plugin
        try plugin_ptr.init_fn(&manager.context);
        
        // Register plugin
        try manager.plugins.append(plugin_ptr);
    }
    
    pub fn notifyInsert(manager: *PluginManager, key: []const u8, value: []const u8) !void {
        for (manager.plugins.items) |plugin| {
            if (plugin.on_insert) |callback| {
                try callback(&manager.context, key, value);
            }
        }
    }
};
```

### Export/Import Extensions

**Custom Data Format Support**:
```zig
pub const DataExporter = struct {
    format_handlers: std.AutoHashMap([]const u8, FormatHandler),
    
    pub fn registerFormat(exporter: *DataExporter, format_name: []const u8, handler: FormatHandler) !void {
        try exporter.format_handlers.put(format_name, handler);
    }
    
    pub fn export(exporter: *DataExporter, db: *BrowserDB, format: []const u8, output_path: []const u8) !void {
        const handler = exporter.format_handlers.get(format) orelse 
            return error.UnsupportedFormat;
        
        const output_file = try std.fs.cwd().createFile(output_path, .{});
        defer output_file.close();
        
        // Let handler perform export
        try handler.exportDatabase(db, output_file);
    }
};

pub const FormatHandler = struct {
    exportDatabase: fn (*BrowserDB, std.fs.File) !void,
    importDatabase: fn (std.fs.File, *BrowserDB) !void,
    
    // JSON format example
    pub const JSON = FormatHandler{
        .exportDatabase = exportToJSON,
        .importDatabase = importFromJSON,
    };
};

fn exportToJSON(db: *BrowserDB, output_file: std.fs.File) !void {
    var writer = std.io.BufferedWriter(std.fs.File.Writer).init(output_file.writer());
    defer writer.flush();
    
    try writer.writeAll("[\n");
    
    var first = true;
    for (db.prefix("")) |result| {
        const (key, value) = try result;
        
        if (!first) try writer.writeAll(",\n");
        first = false;
        
        const key_str = try std.fmt.allocPrint(std.heap.page_allocator, "{}", .{std.mem.span(key)});
        defer std.heap.page_allocator.free(key_str);
        
        try writer.print("  {{\"key\": \"{}\", \"value\": \"{}\"}}\n", .{
            key_str,
            std.mem.span(value),
        });
    }
    
    try writer.writeAll("]\n");
}
```

---

<div align="center">

**[⬅️ Back to User Manual](USER_MANUAL.md)** | **[📁 File Structure](FILE_STRUCTURE.md)** | **[🔧 API Reference](API_REFERENCE.md)**

Ready to contribute? Start with the [development setup](#development-setup)!

</div>