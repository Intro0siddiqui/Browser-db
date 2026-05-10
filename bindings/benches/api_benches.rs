use criterion::{criterion_group, criterion_main, Criterion};
use browserdb::{BrowserDB, LocalStoreEntry};
use tempfile::tempdir;
use rand::Rng;

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

criterion_group!(benches, bench_api_insert_with_index, bench_query_builder_latency);
criterion_main!(benches);
