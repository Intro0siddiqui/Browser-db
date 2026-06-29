# Double-Buffered Memtable Implementation Plan

## Design: Per-Shard Flush with Dedicated Flush Thread

### Architecture

```
Writer Thread                          Flush Thread (new)
============                           ==================

put(key, val)                          Loop:
  |                                     wait on Condvar or timeout
  |-- memtable[shard].write() lock        |
  |-- mem.put()                           |
  |-- if mem.should_flush():              |
  |     swap active → frozen             |
  |     create new empty active           |
  |     signal flush Condvar  ----------> |
  |     release lock                     |
  |-- return immediately                 |
                                        For each shard with frozen buffer:
                                          drain frozen → SSTable::create()
                                          push to Level 0
                                          signal compaction
                                          mark shard as flushed

WAL Truncation (deferred):
  - Track "last truncated sequence" per shard
  - Truncate WAL when ALL shards' frozen buffers are flushed
  - WAL keeps growing during flush, max ~2x memtable size
```

### Data Structure Changes

#### LSMTreeInner (lsm_tree.rs)

```rust
pub struct LSMTreeInner {
    // Existing:
    pub memtable: [RwLock<MemTable>; 16],

    // NEW: Frozen buffers (one per shard)
    pub frozen: [Mutex<Option<MemTable>>; 16],

    // NEW: Flush coordination
    pub flush_pending: AtomicBool,
    pub flush_condvar: Condvar,
    pub flush_mutex: Mutex<()>,

    // NEW: WAL truncation tracking
    pub last_truncated_seq: AtomicU64,
    pub flush_seq: AtomicU64,
}
```

#### MemTable (lsm_tree.rs)

No changes needed. `MemTable` already has `clear()` and `should_flush_tuned()`.

### File Changes

#### 1. `config.rs` — No changes needed

#### 2. `lsm_tree.rs` — Major changes

**New fields on LSMTreeInner:**
- `frozen: [Mutex<Option<MemTable>>; 16]` — per-shard frozen buffer
- `flush_pending: AtomicBool` — signals flush thread
- `flush_condvar: Condvar` — wakes flush thread
- `flush_mutex: Mutex<()>` — condvar pair
- `flush_seq: AtomicU64` — monotonic flush sequence counter
- `last_truncated_seq: AtomicU64` — last WAL truncation point

**Modified `put()` flow:**
```rust
pub fn put(&self, key: &[u8], value: &[u8]) -> io::Result<()> {
    // ... existing backpressure, index, blob logic ...

    // WAL log (already async via channel)
    self.inner.wal.read().log(&mut wal_entry)?;

    // Memtable insert
    let shard = (key[0] % 16) as usize;
    let mut mem = self.inner.memtable[shard].write();
    mem.put(key, stored_value, entry_type, 0);

    // Check flush threshold
    if mem.should_flush_tuned(power_save, low_memory) {
        let frozen_entries = std::mem::take(&mut mem.entries);
        mem.clear();

        // Store frozen buffer
        let seq = self.inner.flush_seq.fetch_add(1, Ordering::SeqCst);
        let frozen_mem = MemTable::from_entries(frozen_entries, mem.max_size, mem.table_type);
        *self.inner.frozen[shard].lock() = Some(frozen_mem);

        drop(mem); // release write lock

        // Signal flush thread
        self.inner.flush_pending.store(true, Ordering::SeqCst);
        self.inner.flush_condvar.notify_one();
    }

    Ok(())
}
```

**New `flush_worker()` method:**
```rust
fn flush_worker(inner: Arc<LSMTreeInner>) {
    loop {
        // Wait for flush signal or 100ms timeout
        {
            let mut pending = inner.flush_mutex.lock();
            while !inner.flush_pending.load(Ordering::SeqCst) {
                let _ = inner.flush_condvar.wait_timeout(pending, Duration::from_millis(100));
            }
            inner.flush_pending.store(false, Ordering::SeqCst);
        }

        // Process each shard's frozen buffer
        for shard in 0..16 {
            let frozen = inner.frozen[shard].lock().take();
            if let Some(mem) = frozen {
                if mem.is_empty() { continue; }

                // Drain frozen memtable
                let entries: BTreeMap<_, _> = mem.entries.into_iter().collect();

                // Create SSTable
                let sstable = Arc::new(SSTable::create(
                    0, &entries, &inner.base_path,
                    inner.table_type, None,
                    inner.config.lsm_tree.verify_checksums
                ).unwrap());

                // Push to Level 0
                {
                    let mut l0 = inner.levels[0].write();
                    l0.push(sstable);
                }

                // Trigger compaction
                inner.clone().trigger_compaction(0);
            }
        }

        // WAL truncation: check if all shards are flushed
        // (implementation details TBD)
    }
}
```

**New `Drop` impl:**
```rust
impl Drop for LSMTree {
    fn drop(&mut self) {
        // Signal flush thread to stop
        self.inner.shutdown.store(true, Ordering::SeqCst);
        self.inner.flush_condvar.notify_one();

        // Flush remaining active memtables synchronously
        for shard in 0..16 {
            let mut mem = self.inner.memtable[shard].write();
            if !mem.is_empty() {
                let entries = std::mem::take(&mut mem.entries);
                mem.clear();
                // ... create SSTable synchronously ...
            }
        }

        // Flush any remaining frozen buffers
        for shard in 0..16 {
            let frozen = self.inner.frozen[shard].lock().take();
            if let Some(mem) = frozen {
                // ... create SSTable synchronously ...
            }
        }

        // Wait for flush thread to finish
        // (join handle needed)
    }
}
```

#### 3. `wal.rs` — Minor changes

**New method:**
```rust
pub fn truncate_after(&self, seq: u64) -> io::Result<()> {
    // Truncate WAL entries up to sequence number
    // Called by flush thread after all shards' frozen buffers are flushed
}
```

#### 4. Benchmarks — New benchmarks

**`optimization_benches.rs` additions:**
- `async_flush_throughput` — 1000 puts with async flush (compare to sync)
- `per_shard_flush_contention` — 16 threads each writing to different shards
- `write_during_flush` — writes while flush thread is running

### Implementation Order

1. **Add frozen buffer fields** to LSMTreeInner
2. **Modify put()** to swap active → frozen on threshold
3. **Add flush_worker()** thread
4. **Add Drop impl** for graceful shutdown
5. **Add WAL truncation coordination**
6. **Write benchmarks** for async flush
7. **Convert integration tests** from #[tokio::test] to #[test]
8. **Remove tokio dev-dependency**

### Risks

1. **WAL growth**: WAL may grow to ~2x memtable size during flush. Acceptable for browser DB.
2. **Concurrent flushes**: Need AtomicBool to prevent multiple flush threads. Currently single flush thread, so safe.
3. **Crash recovery**: WAL truncation deferred until flush succeeds. Crash during flush = WAL has all data, SSTable is corrupt → discard SSTable, replay WAL.
4. **Memory usage**: Active + frozen buffers = 2x memtable size. With 20MB default, that's 40MB. Acceptable.

### Testing Strategy

1. Unit tests: flush_worker processes frozen buffers correctly
2. Integration tests: put() under load triggers async flush without blocking
3. Benchmarks: compare write throughput with sync vs async flush
4. Stress test: 16 threads writing simultaneously, verify no data loss
