use browserdb::BrowserDB;
use std::sync::Arc;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // 1. Initialize
    let db = BrowserDB::open("sql_demo.bdb")?;
    // Wrap in Arc to share ownership with SqlEngine if needed, 
    // but BrowserDB::sql(self: Arc<Self>) consumes the Arc.
    // So we must wrap it first.
    let db_arc = Arc::new(db);
    let sql = db_arc.clone().sql();

    println!("🚀 Starting SQL Subsystem Demo");

    // 2. Create Table
    let create_query = "CREATE TABLE users (id INT PRIMARY_KEY, name TEXT, active BOOL)";
    match sql.execute(create_query) {
        Ok(msg) => println!("✅ {}", msg),
        Err(e) => println!("❌ Create Failed: {}", e),
    }

    // 3. Insert Data
    let insert_query1 = "INSERT INTO users VALUES (1, 'Alice', true)";
    match sql.execute(insert_query1) {
        Ok(msg) => println!("✅ {}", msg),
        Err(e) => println!("❌ Insert Failed: {}", e),
    }

    let insert_query2 = "INSERT INTO users VALUES (2, 'Bob', false)";
    match sql.execute(insert_query2) {
        Ok(msg) => println!("✅ {}", msg),
        Err(e) => println!("❌ Insert Failed: {}", e),
    }

    // 4. Select Data
    println!("\n🔍 Querying Data:");
    let select_query = "SELECT * FROM users WHERE id = 1";
    match sql.execute(select_query) {
        Ok(result) => println!("   Result: {}", result),
        Err(e) => println!("❌ Select Failed: {}", e),
    }

    // 5. Verify Persistence (Optional - just reopening would prove it)
    // Since we are using LocalStore under the hood, it persists automatically.

    Ok(())
}
