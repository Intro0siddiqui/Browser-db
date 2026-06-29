use criterion::{criterion_group, criterion_main, Criterion, BatchSize};
use tempfile::tempdir;
use rand::Rng;

fn bench_browserdb_put(c: &mut Criterion) {
    use browserdb::core::lsm_tree::LSMTree;
    use browserdb::core::format::TableType;
    use browserdb::core::config::BrowserDBConfig;

    let dir = tempdir().unwrap();
    let tree = LSMTree::new(dir.path(), TableType::History, 1024 * 1024, BrowserDBConfig::default()).unwrap();
    let mut rng = rand::thread_rng();

    c.bench_function("browserdb_put", |b| {
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

fn bench_browserdb_get(c: &mut Criterion) {
    use browserdb::core::lsm_tree::LSMTree;
    use browserdb::core::format::TableType;
    use browserdb::core::config::BrowserDBConfig;

    let dir = tempdir().unwrap();
    let mut config = BrowserDBConfig::default();
    config.lsm_tree.verify_checksums = false;
    let tree = LSMTree::new(dir.path(), TableType::History, 1024 * 1024, config).unwrap();

    for i in 0..10000 {
        tree.put(format!("key_{:06}", i).into_bytes(), vec![0u8; 128]).unwrap();
    }
    tree.flush().unwrap();

    let mut rng = rand::thread_rng();
    c.bench_function("browserdb_get", |b| {
        b.iter(|| {
            let key = format!("key_{:06}", rng.gen_range(0..10000)).into_bytes();
            let _ = tree.get(&key);
        })
    });
}

fn bench_leveldb_put(c: &mut Criterion) {
    use rusty_leveldb::{DB, Options, WriteBatch};

    let dir = tempdir().unwrap();
    let mut opts = Options::default();
    opts.create_if_missing = true;
    let mut db = DB::open(dir.path().join("leveldb"), opts).unwrap();
    let mut rng = rand::thread_rng();

    c.bench_function("leveldb_put", |b| {
        b.iter_batched(
            || {
                let key: Vec<u8> = (0..16).map(|_| rng.gen()).collect();
                let value: Vec<u8> = (0..256).map(|_| rng.gen()).collect();
                (key, value)
            },
            |(key, value)| {
                let mut batch = WriteBatch::default();
                batch.put(&key, &value);
                db.write(batch, false).unwrap();
            },
            BatchSize::SmallInput,
        )
    });
}

fn bench_leveldb_get(c: &mut Criterion) {
    use rusty_leveldb::{DB, Options, WriteBatch};

    let dir = tempdir().unwrap();
    let mut opts = Options::default();
    opts.create_if_missing = true;
    let mut db = DB::open(dir.path().join("leveldb"), opts).unwrap();

    {
        let mut batch = WriteBatch::default();
        for i in 0..10000 {
            let key = format!("key_{:06}", i).into_bytes();
            let value = vec![0u8; 128];
            batch.put(&key, &value);
        }
        db.write(batch, false).unwrap();
    }

    let mut rng = rand::thread_rng();
    c.bench_function("leveldb_get", |b| {
        b.iter(|| {
            let key = format!("key_{:06}", rng.gen_range(0..10000)).into_bytes();
            let _ = db.get(&key).unwrap();
        })
    });
}

fn bench_sqlite_put(c: &mut Criterion) {
    use rusqlite::{Connection, params};

    let dir = tempdir().unwrap();
    let conn = Connection::open(dir.path().join("test.db")).unwrap();
    conn.execute_batch("CREATE TABLE kv (key BLOB PRIMARY KEY, value BLOB)").unwrap();
    conn.execute_batch("PRAGMA journal_mode=WAL").unwrap();

    let mut rng = rand::thread_rng();

    c.bench_function("sqlite_put", |b| {
        b.iter_batched(
            || {
                let key: Vec<u8> = (0..16).map(|_| rng.gen()).collect();
                let value: Vec<u8> = (0..256).map(|_| rng.gen()).collect();
                (key, value)
            },
            |(key, value)| {
                conn.execute(
                    "INSERT OR REPLACE INTO kv (key, value) VALUES (?1, ?2)",
                    params![key, value],
                ).unwrap();
            },
            BatchSize::SmallInput,
        )
    });
}

fn bench_sqlite_get(c: &mut Criterion) {
    use rusqlite::{Connection, params};

    let dir = tempdir().unwrap();
    let conn = Connection::open(dir.path().join("test.db")).unwrap();
    conn.execute_batch("CREATE TABLE kv (key BLOB PRIMARY KEY, value BLOB)").unwrap();
    conn.execute_batch("PRAGMA journal_mode=WAL").unwrap();

    {
        let mut stmt = conn.prepare("INSERT OR REPLACE INTO kv (key, value) VALUES (?1, ?2)").unwrap();
        for i in 0..10000 {
            let key = format!("key_{:06}", i).into_bytes();
            let value = vec![0u8; 128];
            stmt.execute(params![key, value]).unwrap();
        }
    }

    let mut rng = rand::thread_rng();
    c.bench_function("sqlite_get", |b| {
        b.iter(|| {
            let key = format!("key_{:06}", rng.gen_range(0..10000)).into_bytes();
            let mut stmt = conn.prepare("SELECT value FROM kv WHERE key = ?1").unwrap();
            let _ = stmt.query_row(params![key], |row| row.get::<_, Vec<u8>>(0)).unwrap();
        })
    });
}

criterion_group!(
    benches,
    bench_browserdb_put,
    bench_browserdb_get,
    bench_leveldb_put,
    bench_leveldb_get,
    bench_sqlite_put,
    bench_sqlite_get
);
criterion_main!(benches);
