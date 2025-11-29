use browserdb::{BrowserDB, LocalStoreEntry};
use std::sync::Arc;
use std::time::Instant;
use std::fs;
use std::path::Path;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let db_path = "sql_vs_core_bench.bdb";
    if Path::new(db_path).exists() {
        fs::remove_dir_all(db_path)?;
    }

    let db = BrowserDB::open(db_path)?;
    let db_arc = Arc::new(db);
    let sql = db_arc.clone().sql();

    const COUNT: usize = 50_000;
    println!("⚔️  BrowserDB: Core vs SQL Performance Test");
    println!("   Records: {}", COUNT);
    println!("---------------------------------------------");

    // --- 1. Raw Core Performance ---
    println!("\n🔥 Phase 1: Raw Core (Direct KV)");
    
    let start_core = Instant::now();
    for i in 0..COUNT {
        let entry = LocalStoreEntry {
            origin_hash: 12345,
            key: format!("key_{}", i),
            value: "some_data_value".to_string(),
        };
        // We use localstore table wrapper which does bincode serialization
        db_arc.localstore().insert(&entry)?;
    }
    let duration_core = start_core.elapsed();
    
    println!("   ✅ Time: {:.4}s", duration_core.as_secs_f64());
    println!("   🚀 Throughput: {:.0} ops/sec", COUNT as f64 / duration_core.as_secs_f64());


    // --- 2. SQL Subsystem Performance ---
    println!("\n🧩 Phase 2: SQL Subsystem (Parse + Overhead)");
    
    // Create table first
    sql.execute("CREATE TABLE bench_table (id INT PRIMARY_KEY, val TEXT)")?;

    let start_sql = Instant::now();
    for i in 0..COUNT {
        // Overhead includes: String formatting, Parsing, HashMap creation, Bincode serialization
        let query = format!("INSERT INTO bench_table VALUES ({}, 'some_data_value')", i);
        sql.execute(&query)?;
    }
    let duration_sql = start_sql.elapsed();

    println!("   ✅ Time: {:.4}s", duration_sql.as_secs_f64());
    println!("   🐢 Throughput: {:.0} ops/sec", COUNT as f64 / duration_sql.as_secs_f64());

    // --- Comparison ---
    let ratio = (COUNT as f64 / duration_core.as_secs_f64()) / (COUNT as f64 / duration_sql.as_secs_f64());
    println!("\n📊 Analysis:");
    println!("   Raw Core is {:.1}x faster than SQL Layer", ratio);
    println!("   (Cost of SQL parsing & dynamic typing overhead)");

    Ok(())
}
