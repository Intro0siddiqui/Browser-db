use criterion::{criterion_group, criterion_main, Criterion, BatchSize};
use browserdb::core::lsm_tree::{LSMTree};
use browserdb::core::format::TableType;
use browserdb::core::config::BrowserDBConfig;
use tempfile::tempdir;
use rand::Rng;

fn bench_wal_throughput(c: &mut Criterion) {
    let dir = tempdir().unwrap();
    let tree = LSMTree::new(dir.path(), TableType::History, 1024 * 1024, BrowserDBConfig::default()).unwrap();
    let mut rng = rand::thread_rng();

    c.bench_function("put_with_channel_wal", |b| {
        b.iter_batched(
            || {
                let key: Vec<u8> = (0..16).map(|_| rng.gen()).collect();
                let value: Vec<u8> = (0..256).map(|_| rng.gen()).collect();
                (key, value)
            },
            |(key, value)| {
                tree.put(key, value).unwrap();
            },
            BatchSize::SmallInput,
        )
    });
}

fn bench_read_no_crc(c: &mut Criterion) {
    let dir = tempdir().unwrap();
    let mut config = BrowserDBConfig::default();
    config.lsm_tree.verify_checksums = false;
    let tree = LSMTree::new(dir.path(), TableType::History, 1024 * 1024, config).unwrap();

    for i in 0..10000 {
        tree.put(format!("key_{:06}", i).into_bytes(), vec![0u8; 128]).unwrap();
    }
    tree.flush().unwrap();

    let mut rng = rand::thread_rng();
    c.bench_function("get_no_crc_verification", |b| {
        b.iter(|| {
            let key = format!("key_{:06}", rng.gen_range(0..10000)).into_bytes();
            let _ = tree.get(&key);
        })
    });
}

fn bench_read_with_crc(c: &mut Criterion) {
    let dir = tempdir().unwrap();
    let mut config = BrowserDBConfig::default();
    config.lsm_tree.verify_checksums = true;
    let tree = LSMTree::new(dir.path(), TableType::History, 1024 * 1024, config).unwrap();

    for i in 0..10000 {
        tree.put(format!("key_{:06}", i).into_bytes(), vec![0u8; 128]).unwrap();
    }
    tree.flush().unwrap();

    let mut rng = rand::thread_rng();
    c.bench_function("get_with_crc_verification", |b| {
        b.iter(|| {
            let key = format!("key_{:06}", rng.gen_range(0..10000)).into_bytes();
            let _ = tree.get(&key);
        })
    });
}

fn bench_write_throughput(c: &mut Criterion) {
    let dir = tempdir().unwrap();
    let tree = LSMTree::new(dir.path(), TableType::History, 1024 * 1024, BrowserDBConfig::default()).unwrap();

    c.bench_function("write_throughput_100", |b| {
        b.iter(|| {
            for i in 0..100 {
                let key = format!("k{:08}", i).into_bytes();
                let value = vec![0u8; 64];
                tree.put(key, value).unwrap();
            }
        })
    });
}

criterion_group!(benches, bench_wal_throughput, bench_read_no_crc, bench_read_with_crc, bench_write_throughput);
criterion_main!(benches);
