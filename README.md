# BrowserDB 🚀
**A specialized, zero-copy Relational KV engine designed to bypass the JavaScript FFI boundary tax.**

BrowserDB is a high-performance, embedded storage engine written in pure Rust. It was built specifically to eliminate the SQL parsing bloat and synchronous disk-lock latency (`fsync`) that bottlenecks traditional embedded databases (like SQLite) when accessed via FFI in runtimes like **Bun** and **Deno**.

[Rust] [Bun] [Deno] [License: GPL-v3]

## ⚡ The FFI Bottleneck (And How We Fix It)

When modern JS apps use native databases, passing strings across the FFI boundary triggers heavy memory-copying, SQL parsing, and B-Tree restructuring. BrowserDB drops this overhead to zero using **Mechanical Sympathy**:

* **Zero-Copy Pointers:** Pass memory pointers directly from JS to the Rust core.
* **Asynchronous WAL Group-Commits:** A background thread safely batches and syncs the Write-Ahead Log (WAL) every 5ms, giving you the speed of RAM with hard-disk durability.
* **WiscKey Blob Separation:** Small indexed keys are kept in sharded MemTables, while massive string payloads (like 100KB JSON or text) are routed directly to blob storage, cutting FFI copy times entirely.

## 📊 Cross-Runtime Benchmarks (10,000 Inserts)

BrowserDB achieves ~50μs per individual write, outperforming native SQLite synchronous inserts by over 50x in JavaScript environments.

| Test Case / Metric | Bun Runtime | Deno Runtime | Native SQLite |
| :--- | :--- | :--- | :--- |
| **10k Individual Inserts** | **50.68 ms** | **66.08 ms** | *~2690.19 ms* |
| **Large Blob (100KB) Write** | **0.87 ms** | **0.57 ms** | *~1.17 ms* |
| **1M FFI Call Leak Test** | **PASSED** (Flat) | **PASSED** (Flat) | *N/A* |
| **SIGKILL Crash Recovery** | **PASSED** | **PASSED** | *Safe* |

*(For complete runtime testing scripts, see our integration repo: [browserdb-runtime-bench](https://github.com/Intro0siddiqui/browserdb-runtime-bench))*

## 🏗️ Architecture Under the Hood

* **16-Shard Concurrent MemTables:** Protected by `parking_lot::RwLock` for massive parallel throughput.
* **Fail-Fast Integrity:** 4KB Block Checksums (CRC32/xxHash) prevent disk rot from returning corrupted pointers.
* **Crash-Resilient WAL:** Append-only log with clean tail-discard logic to survive hard `SIGKILL` kernel terminations.
* **HeatMap Indexing:** Intelligent access-frequency tracking for compaction and cache priority.

## 🚀 Quick Start (JS/TS Runtimes)

BrowserDB is designed to be called directly via native FFI. No heavy ORMs, no SQL strings.

```typescript
import { dlopen, FFIType, suffix } from "bun:ffi";

// 1. Load the compiled Rust binary
const { symbols } = dlopen(`./libbrowserdb.${suffix}`, {
  browserdb_insert: { 
    args: [FFIType.ptr, FFIType.cstring, FFIType.cstring], 
    returns: FFIType.i32 
  }
});

// 2. Call it directly like a native JS function (Zero-copy)
symbols.browserdb_insert(
  dbPointer, 
  Buffer.from("user_101\0"), 
  Buffer.from("High-performance payload data...\0")
);
🛠️ Building from Source (Rust)
If you want to use the pure Rust API or build the dynamic libraries for your OS:
git clone [https://github.com/Intro0siddiqui/Browser-db.git](https://github.com/Intro0siddiqui/Browser-db.git)
cd Browser-db/bindings

# Run the core Rust stress tests
cargo run --release --example stress_test

🔒 License
GNU General Public License v3.0 (GPL-3.0)
