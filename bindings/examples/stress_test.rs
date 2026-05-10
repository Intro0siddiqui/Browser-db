use browserdb::{BrowserDB, HistoryEntry};
use rand::Rng;
use std::time::Instant;
use std::path::Path;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().collect();
    if args.len() > 1 && args[1] == "--child-write" {
        let path = &args[2];
        let id: u32 = args[3].parse()?;
        let db = BrowserDB::open(path)?;
        let entry = HistoryEntry {
            timestamp: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)?
                .as_millis(),
            url: format!("https://example.com/page/{}", id),
            url_hash: id as u128,
            title: format!("Crash Recovery Title {}", id),
            visit_count: 1,
        };
        db.history().insert(&entry)?;
        // Abrupt exit
        std::process::exit(0);
    }

    // Setup
    let db_path = "/tmp/stress_test.bdb";
    if Path::new(db_path).exists() {
        std::fs::remove_dir_all(db_path)?;
    }

    println!("🔥 Starting BrowserDB Stress Test");
    println!("================================");

    // 1. Database Initialization
    let start_init = Instant::now();
    let db = BrowserDB::open(db_path)?;
    println!("✅ Database initialized in {:?}", start_init.elapsed());

    // Configuration
    const TOTAL_RECORDS: u32 = 10_000;
    // HistoryEntry payload is roughly: 8 (u64) + 16 (u128) + ~30 (String) + 4 (u32) = ~60 bytes + overhead.
    
    println!("\n📝 Phase 1: Write Stress Test");
    println!("   Target: {} records", TOTAL_RECORDS);
    
    let mut rng = rand::thread_rng();
    let start_write = Instant::now();
    
    for i in 0..TOTAL_RECORDS {
        let entry = HistoryEntry {
            timestamp: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)?
                .as_millis(),
            url: format!("https://example.com/page/{}", i),
            url_hash: i as u128, // Sequential for verification, could be random
            title: format!("Page Title for Record ID {}", i),
            visit_count: rng.gen_range(1..100),
        };

        db.history().insert(&entry)?;

        if (i + 1) % 100_000 == 0 {
            print!("\r   Progress: {}/{} ({:.1}%)", i + 1, TOTAL_RECORDS, ((i + 1) as f64 / TOTAL_RECORDS as f64) * 100.0);
            use std::io::Write;
            std::io::stdout().flush()?;
        }
    }
    
    let write_duration = start_write.elapsed();
    println!("\n   ✅ Write Complete!");
    println!("   Time: {:.2?}", write_duration);
    println!("   Throughput: {:.0} ops/sec", TOTAL_RECORDS as f64 / write_duration.as_secs_f64());

    // 2. Read Stress Test (Random Access)
    println!("\n📖 Phase 2: Random Read Stress Test");
    const READ_COUNT: u32 = 50_000;
    println!("   Target: {} random lookups", READ_COUNT);

    let start_read = Instant::now();
    let mut found_count = 0;

    for _ in 0..READ_COUNT {
        let target_id = rng.gen_range(0..TOTAL_RECORDS);
        if let Some(entry) = db.history().get(target_id as u128)? {
            if entry.url_hash == target_id as u128 {
                found_count += 1;
            }
        }
    }

    let read_duration = start_read.elapsed();
    println!("   ✅ Read Complete!");
    println!("   Found: {}/{}", found_count, READ_COUNT);
    println!("   Time: {:.2?}", read_duration);
    println!("   Throughput: {:.0} ops/sec", READ_COUNT as f64 / read_duration.as_secs_f64());

    // 3. Persistence Verification
    println!("\n💾 Phase 3: Persistence Verification");
    println!("   Closing and Re-opening database...");
    drop(db); // Force close/flush

    let reopen_start = Instant::now();
    let db2 = BrowserDB::open(db_path)?;
    println!("   Re-opened in {:.2?}", reopen_start.elapsed());

    let verify_id = TOTAL_RECORDS / 2;
    if let Some(entry) = db2.history().get(verify_id as u128)? {
        println!("   ✅ Verified Record #{}: '{}'", verify_id, entry.title);
    } else {
        println!("   ❌ FAILED to verify Record #{}", verify_id);
    }

    // 4. Crash Recovery Simulation
    println!("\n💥 Phase 4: Crash Recovery Simulation");
    println!("   Writing records without closing (simulating crash)...");

    let crash_id = TOTAL_RECORDS + 1;

    // Instead of std::mem::forget, we'll spawn a child process that writes and exits abruptly
    let exe = std::env::current_exe()?;
    let status = std::process::Command::new(exe)
        .arg("--child-write")
        .arg(db_path)
        .arg(crash_id.to_string())
        .status()?;

    if !status.success() {
        // Child exited abruptly as expected or failed
        println!("   Child process exited (simulated crash)");
    }

    println!("   Re-opening database to check WAL recovery...");
    let db3 = BrowserDB::open(db_path)?;
    if let Some(recovered) = db3.history().get(crash_id as u128)? {
        println!("   ✅ Successfully recovered record '{}' from WAL!", recovered.title);
    } else {
        println!("   ❌ FAILED to recover record from WAL.");
    }

    println!("\n🎉 Stress Test Finished.");
    Ok(())
}
