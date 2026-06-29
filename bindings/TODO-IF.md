# TODO-IF.md — Deferred Decisions

Items that need user decision before implementation.

---

## 1. sync_all() During Async Flush

**Question**: Should the background flush thread use sync_all() during SSTable creation?

**Current behavior**: `SSTable::create()` calls `file.sync_all()` twice — once after writing entries, once after writing index/checksum.

**Option A (default)**: Keep sync_all() — ~10-50ms per flush, but guarantees crash safety. Writer thread is not blocked (flush runs in background thread).

**Option B**: Skip sync_all() — ~1-5ms per flush, but data may be lost on power failure if OS hasn't flushed page cache before crash. Requires WAL to be kept until explicit durability confirmation.

**Worst case for Option B**: Laptop battery dies 1ms after WAL truncation but before OS flushes SSTable to disk → data lost.

**Recommendation**: Option A (keep sync_all()). Browser DBs on laptops face real power-loss risk.

**Status**: ⏸️ DEFERRED — waiting for user decision.

---

## ~~2. LZ4 Compression~~ — DROPPED

Not justified for browser DB. Storage is not a bottleneck, CPU/battery matters more.

---

## ~~3. Per-Shard Flush Coordination~~ — DONE

Implemented. Each shard has its own frozen buffer. WAL truncation deferred until all shards' frozen buffers are flushed.
