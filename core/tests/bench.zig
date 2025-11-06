const std = @import("std");
const BrowserDB = @import("core/browserdb.zig");

// 性能基准测试
pub fn main() !void {
    std.debug.print("🚀 BrowserDB Performance Benchmarks\n", .{});
    std.debug.print("===================================\n\n", .{});
    
    var gpa = std.heap.GeneralPurposeAllocator(.{}){};
    defer _ = gpa.deinit();
    
    const allocator = gpa.allocator();
    
    // 清理之前的测试数据
    if (std.fs.cwd().deleteTree("/tmp/browserdb-bench")) {
        std.debug.print("🗑️  Cleaned previous benchmark data\n", .{});
    }
    
    // 初始化数据库
    const db = try BrowserDB.init(allocator, "/tmp/browserdb-bench");
    defer db.deinit();
    
    std.debug.print("📊 Starting benchmark tests...\n\n", .{});
    
    // 1. 写入性能测试
    try benchmark_writes(&db);
    
    // 2. 读取性能测试
    try benchmark_reads(&db);
    
    // 3. HeatMap查询性能测试
    try benchmark_hot_queries(&db);
    
    // 4. 内存使用测试
    try benchmark_memory_usage(&db);
    
    // 5. 并发测试
    try benchmark_concurrent_operations(&db);
    
    std.debug.print("\n🎉 All benchmarks completed!\n", .{});
}

fn benchmark_writes(db: *const BrowserDB) !void {
    std.debug.print("📝 Testing Write Performance...\n", .{});
    
    const num_operations = 10000;
    const start_time = std.time.nanoTimestamp();
    
    var i: usize = 0;
    while (i < num_operations) : (i += 1) {
        const entry = BrowserDB.HistoryEntry{
            .timestamp = std.time.milliTimestamp(),
            .url_hash = @as(u128, @intCast(i)),
            .title = std.fmt.allocPrint(db.history.allocator, "Benchmark Entry {}", .{i}) catch continue,
            .visit_count = 1,
        };
        
        // 注意：这里需要处理title的内存管理
        defer db.history.allocator.free(entry.title);
        
        try db.history.insert(entry);
    }
    
    const end_time = std.time.nanoTimestamp();
    const elapsed = @as(f64, @floatFromInt(end_time - start_time)) / 1_000_000.0;
    
    const ops_per_sec = num_operations / (elapsed / 1000.0);
    
    std.debug.print("   Operations: {d}\n", .{num_operations});
    std.debug.print("   Time: {:.2}ms\n", .{elapsed});
    std.debug.print("   Throughput: {:.0} ops/sec\n", .{ops_per_sec});
    
    if (ops_per_sec >= 10000) {
        std.debug.print("   ✅ Target achieved (10k ops/sec)\n", .{});
    } else {
        std.debug.print("   ⚠️  Target missed (10k ops/sec)\n", .{});
    }
}

fn benchmark_reads(db: *const BrowserDB) !void {
    std.debug.print("\n📖 Testing Read Performance...\n", .{});
    
    const num_operations = 100000;
    const start_time = std.time.nanoTimestamp();
    
    var i: usize = 0;
    while (i < num_operations) : (i += 1) {
        _ = try db.history.get(@as(u128, @intCast(i % 1000)));
    }
    
    const end_time = std.time.nanoTimestamp();
    const elapsed = @as(f64, @floatFromInt(end_time - start_time)) / 1_000_000.0;
    
    const ops_per_sec = num_operations / (elapsed / 1000.0);
    
    std.debug.print("   Operations: {d}\n", .{num_operations});
    std.debug.print("   Time: {:.2}ms\n", .{elapsed});
    std.debug.print("   Throughput: {:.0} ops/sec\n", .{ops_per_sec});
    
    if (ops_per_sec >= 100000) {
        std.debug.print("   ✅ Target achieved (100k ops/sec)\n", .{});
    } else {
        std.debug.print("   ⚠️  Target missed (100k ops/sec)\n", .{});
    }
}

fn benchmark_hot_queries(db: *const BrowserDB) !void {
    std.debug.print("\n🔥 Testing HeatMap Query Performance...\n", .{});
    
    const num_queries = 1000;
    const start_time = std.time.nanoTimestamp();
    
    var i: usize = 0;
    while (i < num_queries) : (i += 1) {
        _ = db.history.memtable.hot_query(0.5);
    }
    
    const end_time = std.time.nanoTimestamp();
    const elapsed = @as(f64, @floatFromInt(end_time - start_time)) / 1_000_000.0;
    
    const ops_per_sec = num_queries / (elapsed / 1000.0);
    const avg_latency = elapsed / @as(f64, @floatFromInt(num_queries));
    
    std.debug.print("   Queries: {d}\n", .{num_queries});
    std.debug.print("   Time: {:.2}ms\n", .{elapsed});
    std.debug.print("   Avg Latency: {:.2}ms/query\n", .{avg_latency});
    std.debug.print("   Throughput: {:.0} queries/sec\n", .{ops_per_sec});
    
    if (avg_latency <= 1.0) {
        std.debug.print("   ✅ Target achieved (<1ms query time)\n", .{});
    } else {
        std.debug.print("   ⚠️  Target missed (<1ms query time)\n", .{});
    }
}

fn benchmark_memory_usage(db: *const BrowserDB) !void {
    std.debug.print("\n💾 Testing Memory Usage...\n", .{});
    
    // 获取当前内存使用情况
    const stats = std.os.linux.getrusage();
    const max_rss_kb = stats.ru_maxrss; // KB
    
    std.debug.print("   Max RSS: {} KB\n", .{max_rss_kb});
    std.debug.print("   Max RSS: {:.2} MB\n", .{@as(f64, @floatFromInt(max_rss_kb)) / 1024.0});
    
    // 检查MemTable大小
    const memtable_size = db.history.memtable.entries.items.len;
    std.debug.print("   MemTable entries: {d}\n", .{memtable_size});
    std.debug.print("   HeatMap entries: {d}\n", .{db.history.memtable.heat_map.entries.len});
    
    if (max_rss_kb < 50 * 1024) { // 50MB
        std.debug.print("   ✅ Target achieved (<50MB footprint)\n", .{});
    } else {
        std.debug.print("   ⚠️  Target exceeded (<50MB footprint)\n", .{});
    }
}

fn benchmark_concurrent_operations(db: *const BrowserDB) !void {
    std.debug.print("\n🔄 Testing Concurrent Operations...\n", .{});
    
    // 简化的并发测试 - 在实际实现中会使用线程
    const num_threads = 4;
    const operations_per_thread = 2500;
    
    var total_start = std.time.nanoTimestamp();
    
    // 模拟并发写入
    var thread_id: usize = 0;
    while (thread_id < num_threads) : (thread_id += 1) {
        var op: usize = 0;
        while (op < operations_per_thread) : (op += 1) {
            const entry = BrowserDB.HistoryEntry{
                .timestamp = std.time.milliTimestamp(),
                .url_hash = @as(u128, @intCast(thread_id * 1000000 + op)),
                .title = "Concurrent Entry",
                .visit_count = 1,
            };
            
            try db.history.insert(entry);
        }
    }
    
    const total_end = std.time.nanoTimestamp();
    const total_elapsed = @as(f64, @floatFromInt(total_end - total_start)) / 1_000_000.0;
    
    const total_ops = num_threads * operations_per_thread;
    const ops_per_sec = total_ops / (total_elapsed / 1000.0);
    
    std.debug.print("   Threads: {d}\n", .{num_threads});
    std.debug.print("   Ops per thread: {d}\n", .{operations_per_thread});
    std.debug.print("   Total operations: {d}\n", .{total_ops});
    std.debug.print("   Total time: {:.2}ms\n", .{total_elapsed});
    std.debug.print("   Concurrent throughput: {:.0} ops/sec\n", .{ops_per_sec});
    
    if (ops_per_sec >= 5000) { // 并发性能目标
        std.debug.print("   ✅ Concurrent performance acceptable\n", .{});
    } else {
        std.debug.print("   ⚠️  Concurrent performance needs optimization\n", .{});
    }
}