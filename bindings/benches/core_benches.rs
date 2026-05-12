use criterion::{criterion_group, criterion_main, Criterion, BatchSize};
use browserdb::core::lsm_tree::{LSMTree, MemTable};
use browserdb::core::format::{EntryType, TableType, BDBLogEntry};
use browserdb::core::wal::WALManager;
use browserdb::core::config::BrowserDBConfig;
use tempfile::tempdir;
use rand::Rng;
use std::sync::Arc;
use std::thread;
use std::sync::atomic::{AtomicBool, Ordering};

fn bench_memtable_insertion(c: &mut Criterion) {
    let mut memtable = MemTable::new(1024 * 1024, TableType::LocalStore);
    let mut rng = rand::thread_rng();

    c.bench_function("memtable_insert", |b| {
        b.iter_batched(
            || {
                let key: Vec<u8> = (0..16).map(|_| rng.gen()).collect();
                let value: Vec<u8> = (0..64).map(|_| rng.gen()).collect();
                (key, value)
            },
            |(key, value)| {
                memtable.put(key, value, EntryType::Insert);
            },
            BatchSize::SmallInput,
        )
    });
}

fn bench_wal_logging(c: &mut Criterion) {
    let dir = tempdir().unwrap();
    let wal_path = dir.path().join("test.wal");
    let mut wal = WALManager::new(&wal_path).unwrap();
    let mut rng = rand::thread_rng();

    c.bench_function("wal_log_sync", |b| {
        b.iter_batched(
            || {
                let key: Vec<u8> = (0..16).map(|_| rng.gen()).collect();
                let value: Vec<u8> = (0..64).map(|_| rng.gen()).collect();
                BDBLogEntry::new(EntryType::Insert, key, value)
            },
            |mut entry| {
                wal.log(&mut entry).unwrap();
            },
            BatchSize::SmallInput,
        )
    });
}

fn bench_compaction_speed(c: &mut Criterion) {
    let dir = tempdir().unwrap();
    let tree = LSMTree::new(dir.path(), TableType::LocalStore, 1024 * 1024, BrowserDBConfig::default()).unwrap();

    // Populate some data
    for i in 0..1000 {
        tree.put(format!("key_{:04}", i).into_bytes(), vec![0u8; 64]).unwrap();
    }
    tree.flush().unwrap();

    let l0 = tree.inner.levels[0].read();
    let sstable = l0[0].clone();

    c.bench_function("compaction_get_at_index", |b| {
        b.iter(|| {
            for idx in &sstable.index {
                let _ = sstable.get_at_index(idx);
            }
        })
    });

    c.bench_function("compaction_streaming_iterator", |b| {
        b.iter(|| {
            for _ in sstable.iter() {
                // Consume
            }
        })
    });
}

fn bench_compaction_stall(c: &mut Criterion) {
    let dir = tempdir().unwrap();
    let tree = Arc::new(LSMTree::new(dir.path(), TableType::LocalStore, 1024 * 1024, BrowserDBConfig::default()).unwrap());

    let stop_flag = Arc::new(AtomicBool::new(false));
    let tree_bg = Arc::clone(&tree);
    let stop_clone = Arc::clone(&stop_flag);

    // Background thread to simulate a massive write burst triggering compactions continuously
    let bg_handle = thread::spawn(move || {
        let mut rng = rand::thread_rng();
        let mut i = 0;
        while !stop_clone.load(Ordering::Relaxed) {
            let key = format!("bg_key_{:08}", i).into_bytes();
            let value: Vec<u8> = (0..64).map(|_| rng.gen()).collect();
            tree_bg.put(key, value).unwrap();
            i += 1;
        }
    });

    let mut rng = rand::thread_rng();

    // Foreground inserts to measure if they stall
    c.bench_function("foreground_insert_during_compaction", |b| {
        b.iter_batched(
            || {
                let key = format!("fg_key_{}", rng.gen::<u32>()).into_bytes();
                let value: Vec<u8> = (0..64).map(|_| rng.gen()).collect();
                (key, value)
            },
            |(key, value)| {
                tree.put(key, value).unwrap();
            },
            BatchSize::SmallInput,
        )
    });

    stop_flag.store(true, Ordering::Relaxed);
    let _ = bg_handle.join();
}

criterion_group!(benches, bench_memtable_insertion, bench_wal_logging, bench_compaction_speed, bench_compaction_stall);
criterion_main!(benches);
