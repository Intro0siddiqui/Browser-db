use criterion::{criterion_group, criterion_main, Criterion};
use browserdb::{BrowserDB, LocalStoreEntry, DatabaseMode};
use tempfile::tempdir;
use rand::Rng;
use std::sync::Arc;
use std::thread;
use std::sync::atomic::{AtomicBool, Ordering};

fn bench_api_insert_with_index(c: &mut Criterion) {
    let dir = tempdir().unwrap();
    let db = BrowserDB::open(dir.path()).unwrap();
    let table = db.localstore();
    let mut rng = rand::thread_rng();

    c.bench_function("api_insert_no_index", |b| {
        b.iter(|| {
            let entry = LocalStoreEntry {
                origin_hash: rng.gen(),
                key: format!("key_{}", rng.gen::<u32>()),
                value: "some_value".to_string(),
            };
            table.insert(&entry).unwrap();
        })
    });

    c.bench_function("api_insert_with_index", |b| {
        b.iter(|| {
            let entry = LocalStoreEntry {
                origin_hash: rng.gen(),
                key: format!("key_{}", rng.gen::<u32>()),
                value: "some_value".to_string(),
            };
            table.insert_with_index(&entry, &["value"]).unwrap();
        })
    });
}

fn bench_query_builder_latency(c: &mut Criterion) {
    let dir = tempdir().unwrap();
    let db = BrowserDB::open(dir.path()).unwrap();
    let table = db.localstore();

    // Populate data with index
    for i in 0..1000 {
        let entry = LocalStoreEntry {
            origin_hash: 12345,
            key: format!("key_{:04}", i),
            value: if i % 10 == 0 { "target".to_string() } else { "other".to_string() },
        };
        table.insert_with_index(&entry, &["value"]).unwrap();
    }

    c.bench_function("query_predicate_scan", |b| {
        b.iter(|| {
            let _ = table.query()
                .filter(|e| e.value == "target")
                .execute()
                .unwrap();
        })
    });

    c.bench_function("query_value_eq_index", |b| {
        b.iter(|| {
            let _ = table.query()
                .value_eq("target".to_string())
                .execute()
                .unwrap();
        })
    });
}

fn bench_multi_threaded_concurrency(c: &mut Criterion) {
    let dir = tempdir().unwrap();
    let db = Arc::new(BrowserDB::open(dir.path()).unwrap());

    let stop_flag = Arc::new(AtomicBool::new(false));
    let mut handles = vec![];

    // Start 100 background threads to create lock contention
    for i in 0..100 {
        let db_clone = Arc::clone(&db);
        let stop_clone = Arc::clone(&stop_flag);
        handles.push(thread::spawn(move || {
            let table = db_clone.localstore();
            let mut j = 0;
            while !stop_clone.load(Ordering::Relaxed) {
                if i % 2 == 0 {
                    let entry = LocalStoreEntry {
                        origin_hash: 123,
                        key: format!("thread_key_{}_{}", i, j),
                        value: "data".to_string(),
                    };
                    let _ = table.insert(&entry);
                } else {
                    let _ = table.get_by_origin(123);
                }
                j += 1;
            }
        }));
    }

    let table = db.localstore();
    let mut rng = rand::thread_rng();

    c.bench_function("concurrent_rw_100_threads", |b| {
        b.iter(|| {
            let entry = LocalStoreEntry {
                origin_hash: rng.gen(),
                key: format!("fg_key_{}", rng.gen::<u32>()),
                value: "data".to_string(),
            };
            table.insert(&entry).unwrap();
        });
    });

    stop_flag.store(true, Ordering::Relaxed);
    for handle in handles {
        let _ = handle.join();
    }
}

fn bench_mode_comparison(c: &mut Criterion) {
    let dir = tempdir().unwrap();
    let db = BrowserDB::open(dir.path()).unwrap();
    let table = db.localstore();
    let mut rng = rand::thread_rng();

    c.bench_function("mode_comparison_persistent_insert", |b| {
        db.set_mode(DatabaseMode::Persistent).unwrap();
        b.iter(|| {
            let entry = LocalStoreEntry {
                origin_hash: rng.gen(),
                key: format!("key_{}", rng.gen::<u32>()),
                value: "persistent".to_string(),
            };
            table.insert(&entry).unwrap();
        })
    });

    c.bench_function("mode_comparison_ultra_insert", |b| {
        db.set_mode(DatabaseMode::Ultra).unwrap();
        b.iter(|| {
            let entry = LocalStoreEntry {
                origin_hash: rng.gen(),
                key: format!("key_{}", rng.gen::<u32>()),
                value: "ultra".to_string(),
            };
            table.insert(&entry).unwrap();
        })
    });
}

criterion_group!(benches, bench_api_insert_with_index, bench_query_builder_latency, bench_multi_threaded_concurrency, bench_mode_comparison);
criterion_main!(benches);
