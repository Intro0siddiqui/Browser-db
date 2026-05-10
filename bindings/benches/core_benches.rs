use criterion::{criterion_group, criterion_main, Criterion};
use browserdb::core::lsm_tree::{LSMTree, MemTable};
use browserdb::core::format::{EntryType, TableType, BDBLogEntry};
use browserdb::core::wal::WALManager;
use tempfile::tempdir;
use rand::Rng;

fn bench_memtable_insertion(c: &mut Criterion) {
    let mut memtable = MemTable::new(1024 * 1024, TableType::LocalStore);
    let mut rng = rand::thread_rng();

    c.bench_function("memtable_insert", |b| {
        b.iter(|| {
            let key: Vec<u8> = (0..16).map(|_| rng.gen()).collect();
            let value: Vec<u8> = (0..64).map(|_| rng.gen()).collect();
            memtable.put(key, value, EntryType::Insert);
        })
    });
}

fn bench_wal_logging(c: &mut Criterion) {
    let dir = tempdir().unwrap();
    let wal_path = dir.path().join("test.wal");
    let mut wal = WALManager::new(&wal_path).unwrap();
    let mut rng = rand::thread_rng();

    c.bench_function("wal_log_sync", |b| {
        b.iter(|| {
            let key: Vec<u8> = (0..16).map(|_| rng.gen()).collect();
            let value: Vec<u8> = (0..64).map(|_| rng.gen()).collect();
            let mut entry = BDBLogEntry::new(EntryType::Insert, key, value);
            wal.log(&mut entry).unwrap();
        })
    });
}

fn bench_compaction_speed(c: &mut Criterion) {
    let dir = tempdir().unwrap();
    let tree = LSMTree::new(dir.path(), TableType::LocalStore, 1024 * 1024);

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

criterion_group!(benches, bench_memtable_insertion, bench_wal_logging, bench_compaction_speed);
criterion_main!(benches);
