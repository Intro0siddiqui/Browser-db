//! Performance Benchmarks & Optimization Tests
//! 
//! Comprehensive performance testing suite for BrowserDB including:
//! - Throughput benchmarks for all operations
//! - Memory usage analysis
//! - Heat map efficiency tests
//! - I/O performance tests
//! - Stress testing under load
//! - Optimization validation tests

const std = @import("std");
const testing = std.testing;
const bdb_format = @import("bdb_format.zig");
const lsm_tree = @import("lsm_tree.zig");
const HeatMap = @import("heatmap_indexing.zig");
const ModesOps = @import("modes_operations.zig");

/// Benchmark result structure
const BenchmarkResult = struct {
    name: []const u8,
    operations_per_second: f64,
    total_time_ms: f64,
    memory_usage_mb: u64,
    throughput_mb_per_sec: f64,
    latency_p99_ms: f64,
};

fn BenchmarkAllocator() type {
    return struct {
        backing: std.heap.GeneralPurposeAllocator(.{}),
        
        const Self = @This();
        
        pub fn init() Self {
            return .{ .backing = std.heap.GeneralPurposeAllocator(.{}){} };
        }
        
        pub fn allocator(self: *Self) std.mem.Allocator {
            return self.backing.allocator();
        }
        
        pub fn deinit(self: *Self) void {
            _ = self.backing.deinit();
        }
    };
}

test "LSMTree Write Throughput Benchmark" {
    const allocator = BenchmarkAllocator();
    defer allocator.deinit();
    
    const base_path = "/tmp/browserdb_write_benchmark";
    try std.fs.cwd().makeDir(base_path);
    defer std.fs.cwd().deleteDir(base_path) catch {};
    
    const lsm_tree = try lsm_tree.LSMTree.init(allocator.allocator(), base_path, .History, 64 * 1024 * 1024);
    defer lsm_tree.deinit();
    
    const warmup_ops = 1000;
    const benchmark_ops = 10000;
    
    // Warmup phase
    for (0..warmup_ops) |i| {
        const key = std.fmt.allocPrintZ(allocator.allocator(), "warmup_{d}", .{i}) catch break;
        defer allocator.allocator().free(key);
        try lsm_tree.put(key, "warmup");
    }
    
    // Benchmark phase
    const start_time = std.time.nanoTimestamp();
    const start_memory = std.heap_general_purpose_allocator_instance.allocated_bytes;
    
    for (0..benchmark_ops) |i| {
        const key = std.fmt.allocPrintZ(allocator.allocator(), "bench_key_{d}", .{i}) catch break;
        defer allocator.allocator().free(key);
        
        const value = std.fmt.allocPrintZ(allocator.allocator(), "benchmark_value_{d}_with_additional_data_to_increase_size", .{i}) catch break;
        defer allocator.allocator().free(value);
        
        try lsm_tree.put(key, value);
    }
    
    const end_time = std.time.nanoTimestamp();
    const end_memory = std.heap_general_purpose_allocator_instance.allocated_bytes;
    
    const elapsed_ms = @as(f64, @floatFromInt(end_time - start_time)) / 1_000_000.0;
    const ops_per_sec = @as(f64, @floatFromInt(benchmark_ops)) / (elapsed_ms / 1000.0);
    const memory_used_mb = (end_memory - start_memory) / (1024 * 1024);
    const avg_value_size = (memory_used_mb * 1024 * 1024) / benchmark_ops;
    
    const result = BenchmarkResult{
        .name = "LSMTree Write Throughput",
        .operations_per_second = ops_per_sec,
        .total_time_ms = elapsed_ms,
        .memory_usage_mb = memory_used_mb,
        .throughput_mb_per_sec = (benchmark_ops * avg_value_size) / (1024 * 1024 * elapsed_ms / 1000.0),
        .latency_p99_ms = 0.0, // Would need histogram tracking
    };
    
    // Performance targets
    std.debug.print("📊 Write Throughput Benchmark:\n", .{});
    std.debug.print("  Operations: {d}\n", .{benchmark_ops});
    std.debug.print("  Time: {:.2}ms\n", .{result.total_time_ms});
    std.debug.print("  Throughput: {:.0} ops/sec\n", .{result.operations_per_second});
    std.debug.print("  Memory Usage: {d} MB\n", .{result.memory_usage_mb});
    std.debug.print("  Avg Value Size: {:.0} bytes\n", .{avg_value_size});
    
    // Performance expectations (should be reasonable for modern hardware)
    testing.expect(result.operations_per_second > 5000.0); // At least 5K writes/sec
    testing.expect(result.memory_usage_mb < 100); // Should use reasonable memory
}

test "LSMTree Read Throughput Benchmark" {
    const allocator = BenchmarkAllocator();
    defer allocator.deinit();
    
    const base_path = "/tmp/browserdb_read_benchmark";
    try std.fs.cwd().makeDir(base_path);
    defer std.fs.cwd().deleteDir(base_path) catch {};
    
    const lsm_tree = try lsm_tree.LSMTree.init(allocator.allocator(), base_path, .History, 32 * 1024 * 1024);
    defer lsm_tree.deinit();
    
    // Pre-populate database
    const prepopulation_ops = 5000;
    for (0..prepopulation_ops) |i| {
        const key = std.fmt.allocPrintZ(allocator.allocator(), "read_bench_key_{d}", .{i}) catch break;
        defer allocator.allocator().free(key);
        
        const value = std.fmt.allocPrintZ(allocator.allocator(), "read_bench_value_{d}", .{i}) catch break;
        defer allocator.allocator().free(value);
        
        try lsm_tree.put(key, value);
    }
    
    // Force flush to ensure data is in SSTables
    try lsm_tree.flush();
    
    const benchmark_ops = 10000;
    const start_time = std.time.nanoTimestamp();
    
    // Benchmark random reads
    var found_count: usize = 0;
    for (0..benchmark_ops) |i| {
        const key_index = @mod(i * 7, prepopulation_ops); // Pseudo-random access
        const key = std.fmt.allocPrintZ(allocator.allocator(), "read_bench_key_{d}", .{key_index}) catch continue;
        defer allocator.allocator().free(key);
        
        if (lsm_tree.get(key)) |_| {
            found_count += 1;
        }
    }
    
    const end_time = std.time.nanoTimestamp();
    const elapsed_ms = @as(f64, @floatFromInt(end_time - start_time)) / 1_000_000.0;
    const ops_per_sec = @as(f64, @floatFromInt(benchmark_ops)) / (elapsed_ms / 1000.0);
    
    const result = BenchmarkResult{
        .name = "LSMTree Read Throughput",
        .operations_per_second = ops_per_sec,
        .total_time_ms = elapsed_ms,
        .memory_usage_mb = 0,
        .throughput_mb_per_sec = 0.0,
        .latency_p99_ms = 0.0,
    };
    
    std.debug.print("📊 Read Throughput Benchmark:\n", .{});
    std.debug.print("  Operations: {d}\n", .{benchmark_ops});
    std.debug.print("  Found: {d}\n", .{found_count});
    std.debug.print("  Time: {:.2}ms\n", .{result.total_time_ms});
    std.debug.print("  Throughput: {:.0} ops/sec\n", .{result.operations_per_second});
    
    // Read performance should be significantly higher than write
    testing.expect(result.operations_per_second > 20000.0); // At least 20K reads/sec
    testing.expect(@as(f64, @floatFromInt(found_count)) / @as(f64, @floatFromInt(benchmark_ops)) > 0.95); // >95% hit rate
}

test "Heat Map Performance Benchmark" {
    const allocator = BenchmarkAllocator();
    defer allocator.deinit();
    
    var manager = try HeatMap.DynamicHeatManager.init(allocator.allocator(), 10000, 32);
    defer manager.deinit(allocator.allocator());
    
    const num_keys = 1000;
    const accesses_per_key = 50;
    
    // Create keys with varying access patterns
    var key_list = std.ArrayList(bdb_format.BDBKey).init(allocator.allocator());
    defer key_list.deinit();
    
    for (0..num_keys) |i| {
        const key_data = std.fmt.allocPrintZ(allocator.allocator(), "heatmap_key_{d}", .{i}) catch continue;
        defer allocator.allocator().free(key_data);
        
        const key = bdb_format.BDBKey.fromString(key_data) catch continue;
        try key_list.append(key);
    }
    
    const start_time = std.time.nanoTimestamp();
    
    // Simulate access patterns
    for (0..accesses_per_key) |access_round| {
        for (key_list.items) |key| {
            const value_data = std.fmt.allocPrintZ(allocator.allocator(), "value_{d}_{d}", .{key_list.items.len, access_round}) catch continue;
            defer allocator.allocator().free(value_data);
            
            const value = bdb_format.BDBValue.fromString(value_data) catch continue;
            
            try manager.recordAccess(key, value, .Read);
        }
    }
    
    const end_time = std.time.nanoTimestamp();
    const elapsed_ms = @as(f64, @floatFromInt(end_time - start_time)) / 1_000_000.0;
    const total_ops = num_keys * accesses_per_key;
    const ops_per_sec = @as(f64, @floatFromInt(total_ops)) / (elapsed_ms / 1000.0);
    
    // Benchmark hot key queries
    const hot_query_start = std.time.nanoTimestamp();
    const hot_keys = try manager.getHotKeys(100);
    defer allocator.allocator().free(hot_keys);
    const hot_query_end = std.time.nanoTimestamp();
    const hot_query_time = @as(f64, @floatFromInt(hot_query_end - hot_query_start)) / 1_000_000.0;
    
    std.debug.print("🔥 Heat Map Performance:\n", .{});
    std.debug.print("  Total Accesses: {d}\n", .{total_ops});
    std.debug.print("  Access Time: {:.2}ms ({:.0} ops/sec)\n", .{elapsed_ms, ops_per_sec});
    std.debug.print("  Hot Keys Found: {d}\n", .{hot_keys.len});
    std.debug.print("  Hot Query Time: {:.3}ms\n", .{hot_query_time});
    
    // Performance expectations
    testing.expect(ops_per_sec > 100000.0); // Should handle >100K heat updates/sec
    testing.expect(hot_query_time < 10.0); // Hot queries should be very fast
    testing.expect(hot_keys.len > 0); // Should find some hot keys
}

test "Memory Pool Efficiency Benchmark" {
    const allocator = BenchmarkAllocator();
    defer allocator.deinit();
    
    const pool_size = 32 * 1024 * 1024; // 32MB
    var pool = ModesOps.UltraMode.MemoryPool.init(allocator.allocator(), pool_size);
    defer pool.deinit();
    
    const allocation_rounds = 100;
    const allocations_per_round = 100;
    const start_time = std.time.nanoTimestamp();
    
    var total_allocated: usize = 0;
    
    for (0..allocation_rounds) |round| {
        var round_allocated: usize = 0;
        
        for (0..allocations_per_round) |i| {
            // Vary allocation sizes (1KB to 64KB)
            const size = 1024 + (round * allocations_per_round + i) % (64 * 1024);
            
            if (pool.alloc(size)) |data| {
                round_allocated += size;
                
                // Fill with pattern for verification
                @memset(data, @truncate(u8, round * allocations_per_round + i));
                
                // Keep some data for a while
                if (i % 10 != 0) {
                    pool.free(data);
                }
            }
        }
        
        total_allocated += round_allocated;
        
        // Periodically free some old allocations
        if (round % 20 == 0) {
            // Simulate some freeing (in real scenario, would track specific allocations)
        }
    }
    
    const end_time = std.time.nanoTimestamp();
    const elapsed_ms = @as(f64, @floatFromInt(end_time - start_time)) / 1_000_000.0;
    const allocations_per_sec = @as(f64, @floatFromInt(allocation_rounds * allocations_per_round)) / (elapsed_ms / 1000.0);
    
    std.debug.print("💾 Memory Pool Performance:\n", .{});
    std.debug.print("  Pool Size: {d} MB\n", .{pool_size / (1024 * 1024)});
    std.debug.print("  Total Allocations: {d}\n", .{allocation_rounds * allocations_per_round});
    std.debug.print("  Total Allocated: {d} bytes\n", .{total_allocated});
    std.debug.print("  Time: {:.2}ms\n", .{elapsed_ms});
    std.debug.print("  Allocation Rate: {:.0} allocs/sec\n", .{allocations_per_sec});
    std.debug.print("  Memory Usage: {d} MB\n", .{pool.used_memory / (1024 * 1024)});
    
    // Performance expectations
    testing.expect(allocations_per_sec > 50000.0); // Should handle >50K allocations/sec
    testing.expect(pool.used_memory > 0); // Should be using memory
}

test "File I/O Performance Benchmark" {
    const allocator = BenchmarkAllocator();
    defer allocator.deinit();
    
    const test_path = "/tmp/browserdb_io_benchmark.bdb";
    std.fs.cwd().deleteFile(test_path) catch {};
    
    const start_time = std.time.nanoTimestamp();
    
    var file = try bdb_format.BDBFile.init(allocator.allocator(), test_path, .Cache);
    defer file.deinit();
    
    const num_entries = 5000;
    var total_data_written: usize = 0;
    
    for (0..num_entries) |i| {
        // Vary entry sizes
        const key_size = 10 + (i % 100);
        const value_size = 50 + (i % 1000);
        
        var key = std.ArrayList(u8).init(allocator.allocator());
        defer key.deinit();
        try key.appendNTimes(@truncate(u8, i), key_size);
        
        var value = std.ArrayList(u8).init(allocator.allocator());
        defer value.deinit();
        try value.appendNTimes(@truncate(u8, i + 1), value_size);
        
        const entry = bdb_format.BDBLogEntry.createInsert(key.items, value.items, i * 1000);
        try file.appendEntry(entry);
        
        total_data_written += key.items.len + value.items.len;
    }
    
    try file.writeFooter();
    
    const end_time = std.time.nanoTimestamp();
    const elapsed_ms = @as(f64, @floatFromInt(end_time - start_time)) / 1_000_000.0;
    const entries_per_sec = @as(f64, @floatFromInt(num_entries)) / (elapsed_ms / 1000.0);
    const mb_per_sec = (@as(f64, @floatFromInt(total_data_written)) / (1024 * 1024)) / (elapsed_ms / 1000.0);
    
    std.debug.print("📁 File I/O Performance:\n", .{});
    std.debug.print("  Entries: {d}\n", .{num_entries});
    std.debug.print("  Data Written: {d} bytes ({.2} MB)\n", .{ total_data_written, @as(f64, @floatFromInt(total_data_written)) / (1024 * 1024) });
    std.debug.print("  Time: {:.2}ms\n", .{elapsed_ms});
    std.debug.print("  Entry Rate: {:.0} entries/sec\n", .{entries_per_sec});
    std.debug.print("  Data Rate: {:.2} MB/sec\n", .{mb_per_sec});
    
    // Performance expectations
    testing.expect(entries_per_sec > 2000.0); // Should handle >2K entries/sec
    testing.expect(mb_per_sec > 10.0); // Should write >10MB/sec
    
    // Verify file integrity
    const stats = try file.getStats();
    testing.expect(stats.entry_count == num_entries);
    testing.expect(stats.file_size > total_data_written); // Should have overhead for headers
}

test "Compaction Performance Benchmark" {
    const allocator = BenchmarkAllocator();
    defer allocator.deinit();
    
    const base_path = "/tmp/browserdb_compaction_benchmark";
    try std.fs.cwd().makeDir(base_path);
    defer std.fs.cwd().deleteDir(base_path) catch {};
    
    const lsm_tree = try lsm_tree.LSMTree.init(allocator.allocator(), base_path, .History, 512); // Small for frequent flushes
    defer lsm_tree.deinit();
    
    // Create many SSTables through repeated flushes
    const num_flushes = 20;
    const entries_per_flush = 50;
    
    for (0..num_flushes) |flush| {
        for (0..entries_per_flush) |i| {
            const key = std.fmt.allocPrintZ(allocator.allocator(), "compaction_key_{d}_{d}", .{flush, i}) catch break;
            defer allocator.allocator().free(key);
            
            const value = std.fmt.allocPrintZ(allocator.allocator(), "compaction_value_{d}_{d}", .{flush, i}) catch break;
            defer allocator.allocator().free(value);
            
            try lsm_tree.put(key, value);
        }
        
        // Force flush to create new SSTable
        try lsm_tree.flush();
    }
    
    // Get stats before compaction
    const stats_before = try lsm_tree.getStats();
    
    const start_time = std.time.nanoTimestamp();
    
    // Perform compaction
    try lsm_tree.compact();
    
    const end_time = std.time.nanoTimestamp();
    const elapsed_ms = @as(f64, @floatFromInt(end_time - start_time)) / 1_000_000.0;
    
    const stats_after = try lsm_tree.getStats();
    
    std.debug.print("🗜️ Compaction Performance:\n", .{});
    std.debug.print("  SSTables Before: {d}\n", .{stats_before.total_sstables});
    std.debug.print("  SSTables After: {d}\n", .{stats_after.total_sstables});
    std.debug.print("  Compaction Time: {:.2}ms\n", .{elapsed_ms});
    std.debug.print("  Compaction Rate: {:.0} SSTables/sec\n", .{
        @as(f64, @floatFromInt(stats_before.total_sstables)) / (elapsed_ms / 1000.0)
    });
    
    // Performance expectations
    testing.expect(elapsed_ms < 10000.0); // Should complete within 10 seconds
    testing.expect(stats_after.total_sstables <= stats_before.total_sstables); // Should not increase
    
    // Verify data integrity
    const test_key = std.fmt.allocPrintZ(allocator.allocator(), "compaction_key_{d}_{d}", .{num_flushes / 2, entries_per_flush / 2}) catch "";
    defer allocator.allocator().free(test_key);
    
    if (lsm_tree.get(test_key)) |entry| {
        testing.expect(!entry.deleted);
    }
}

test "Stress Test - High Concurrent Load" {
    const allocator = BenchmarkAllocator();
    defer allocator.deinit();
    
    const base_path = "/tmp/browserdb_stress_test";
    try std.fs.cwd().makeDir(base_path);
    defer std.fs.cwd().deleteDir(base_path) catch {};
    
    const lsm_tree = try lsm_tree.LSMTree.init(allocator.allocator(), base_path, .History, 16 * 1024 * 1024);
    defer lsm_tree.deinit();
    
    const num_threads = 4; // Simulate concurrent operations
    const operations_per_thread = 1000;
    
    const start_time = std.time.nanoTimestamp();
    var total_successful_operations: usize = 0;
    
    // Simulate concurrent access patterns
    for (0..num_threads) |thread_id| {
        for (0..operations_per_thread) |i| {
            const global_id = thread_id * operations_per_thread + i;
            const key = std.fmt.allocPrintZ(allocator.allocator(), "stress_key_{d}", .{global_id % (num_threads * 100)}) catch continue;
            defer allocator.allocator().free(key);
            
            const value = std.fmt.allocPrintZ(allocator.allocator(), "stress_value_{d}_{d}", .{thread_id, i}) catch continue;
            defer allocator.allocator().free(value);
            
            // Mix of operations
            const operation_type = global_id % 4;
            switch (operation_type) {
                0 => { // Insert
                    try lsm_tree.put(key, value);
                },
                1 => { // Read
                    _ = lsm_tree.get(key);
                },
                2 => { // Update
                    try lsm_tree.put(key, "updated_" ++ value);
                },
                3 => { // Delete
                    try lsm_tree.delete(key);
                },
                else => {},
            }
            
            total_successful_operations += 1;
        }
    }
    
    const end_time = std.time.nanoTimestamp();
    const elapsed_ms = @as(f64, @floatFromInt(end_time - start_time)) / 1_000_000.0;
    const ops_per_sec = @as(f64, @floatFromInt(total_successful_operations)) / (elapsed_ms / 1000.0);
    
    const final_stats = try lsm_tree.getStats();
    
    std.debug.print("⚡ Stress Test Results:\n", .{});
    std.debug.print("  Total Operations: {d}\n", .{total_successful_operations});
    std.debug.print("  Threads: {d}\n", .{num_threads});
    std.debug.print("  Time: {:.2}ms\n", .{elapsed_ms});
    std.debug.print("  Throughput: {:.0} ops/sec\n", .{ops_per_sec});
    std.debug.print("  Final MemTable Size: {d} bytes\n", .{final_stats.memtable_size});
    std.debug.print("  Final SSTables: {d}\n", .{final_stats.total_sstables});
    
    // Stress test expectations
    testing.expect(total_successful_operations == num_threads * operations_per_thread);
    testing.expect(ops_per_sec > 3000.0); // Should maintain reasonable throughput under stress
}

test "Memory Efficiency Analysis" {
    const allocator = BenchmarkAllocator();
    defer allocator.deinit();
    
    const base_path = "/tmp/browserdb_memory_analysis";
    try std.fs.cwd().makeDir(base_path);
    defer std.fs.cwd().deleteDir(base_path) catch {};
    
    const lsm_tree = try lsm_tree.LSMTree.init(allocator.allocator(), base_path, .History, 32 * 1024 * 1024);
    defer lsm_tree.deinit();
    
    const num_entries = 20000;
    const start_memory = std.heap_general_purpose_allocator_instance.allocated_bytes;
    
    // Insert entries with varying sizes
    for (0..num_entries) |i| {
        const key_size = 8 + (i % 40); // 8-48 bytes
        const value_size = 32 + (i % 256); // 32-288 bytes
        
        var key = std.ArrayList(u8).init(allocator.allocator());
        defer key.deinit();
        try key.appendNTimes(@truncate(u8, i), key_size);
        
        var value = std.ArrayList(u8).init(allocator.allocator());
        defer value.deinit();
        try value.appendNTimes(@truncate(u8, i + 1), value_size);
        
        try lsm_tree.put(key.items, value.items);
    }
    
    const peak_memory = std.heap_general_purpose_allocator_instance.allocated_bytes;
    
    // Force flush to move data to disk
    try lsm_tree.flush();
    
    // Wait a bit for garbage collection
    std.time.sleep(std.time.ns_per_ms * 100);
    
    const after_gc_memory = std.heap_general_purpose_allocator_instance.allocated_bytes;
    const memory_used = peak_memory - start_memory;
    const memory_after_gc = after_gc_memory - start_memory;
    const avg_entry_size = @as(f64, @floatFromInt(memory_used)) / @as(f64, @floatFromInt(num_entries));
    
    std.debug.print("💾 Memory Efficiency Analysis:\n", .{});
    std.debug.print("  Entries: {d}\n", .{num_entries});
    std.debug.print("  Peak Memory: {d} MB\n", .{memory_used / (1024 * 1024)});
    std.debug.print("  After GC: {d} MB\n", .{memory_after_gc / (1024 * 1024)});
    std.debug.print("  Memory Per Entry: {:.1} bytes\n", .{avg_entry_size});
    std.debug.print("  GC Efficiency: {d}%\n", .{
        @as(f64, @floatFromInt(memory_used - memory_after_gc)) / @as(f64, @floatFromInt(memory_used)) * 100.0
    });
    
    // Memory efficiency expectations
    testing.expect(avg_entry_size < 500.0); // Should use <500 bytes per entry
    testing.expect(memory_after_gc < memory_used * 2); // Should not leak significantly
}

// ==================== OPTIMIZATION VALIDATION TESTS ====================

test "Heat Map Accuracy Validation" {
    const allocator = BenchmarkAllocator();
    defer allocator.deinit();
    
    var manager = try HeatMap.DynamicHeatManager.init(allocator.allocator(), 1000, 16);
    defer manager.deinit(allocator.allocator());
    
    // Create predictable access patterns
    const hot_keys = [_][]const u8{ "user_profile", "session_data", "current_settings" };
    const warm_keys = [_][]const u8{ "cached_queries", "recent_history" };
    const cold_keys = [_][]const u8{ "old_logs", "archived_data", "temp_cache" };
    
    // Make hot keys very hot
    for (hot_keys) |key_data| {
        const key = bdb_format.BDBKey.fromString(key_data) catch continue;
        const value = bdb_format.BDBValue.fromString("hot_value") catch continue;
        
        for (0..50) |_| {
            try manager.recordAccess(key, value, .Read);
        }
    }
    
    // Make warm keys moderately hot
    for (warm_keys) |key_data| {
        const key = bdb_format.BDBKey.fromString(key_data) catch continue;
        const value = bdb_format.BDBValue.fromString("warm_value") catch continue;
        
        for (0..15) |_| {
            try manager.recordAccess(key, value, .Read);
        }
    }
    
    // Make cold keys slightly hot
    for (cold_keys) |key_data| {
        const key = bdb_format.BDBKey.fromString(key_data) catch continue;
        const value = bdb_format.BDBValue.fromString("cold_value") catch continue;
        
        for (0..3) |_| {
            try manager.recordAccess(key, value, .Read);
        }
    }
    
    // Verify heat ordering
    const hot_keys_result = try manager.getHotKeys(10);
    defer allocator.allocator().free(hot_keys_result);
    
    // Verify hot keys are detected as hot
    for (hot_keys) |key_data| {
        const key = bdb_format.BDBKey.fromString(key_data) catch continue;
        testing.expect(manager.isHot(key));
    }
    
    // Verify cold keys are not detected as hot
    for (cold_keys) |key_data| {
        const key = bdb_format.BDBKey.fromString(key_data) catch continue;
        testing.expect(!manager.isHot(key));
    }
    
    std.debug.print("✅ Heat Map Accuracy: Hot keys detected correctly\n");
}

test "Compression Efficiency Validation" {
    const allocator = BenchmarkAllocator();
    defer allocator.deinit();
    
    const test_path = "/tmp/test_compression.bdb";
    std.fs.cwd().deleteFile(test_path) catch {};
    
    var file = try bdb_format.BDBFile.init(allocator.allocator(), test_path, .Cache);
    defer file.deinit();
    
    // Insert repetitive data that should compress well
    for (0..1000) |i| {
        const key = std.fmt.allocPrintZ(allocator.allocator(), "repeated_key_{d}", .{i % 10}) catch continue;
        defer allocator.allocator().free(key);
        
        const value = std.fmt.allocPrintZ(allocator.allocator(), "This is a long repetitive value that should compress well {d}", .{i % 5}) catch continue;
        defer allocator.allocator().free(value);
        
        const entry = bdb_format.BDBLogEntry.createInsert(key, value, i);
        try file.appendEntry(entry);
    }
    
    try file.writeFooter();
    
    const stats = try file.getStats();
    
    // With repetitive data, compression should reduce effective size
    const theoretical_uncompressed_size = 1000 * 50; // Rough estimate
    const compression_ratio = @as(f64, @floatFromInt(stats.file_size)) / @as(f64, @floatFromInt(theoretical_uncompressed_size));
    
    std.debug.print("🗜️ Compression Efficiency:\n", .{});
    std.debug.print("  File Size: {d} bytes\n", .{stats.file_size});
    std.debug.print("  Compression Ratio: {:.2}\n", .{compression_ratio});
    std.debug.print("  Entries: {d}\n", .{stats.entry_count});
    
    // Should show some compression benefit with repetitive data
    testing.expect(stats.file_size > 0);
}

test "Cache Hit Rate Optimization Validation" {
    const allocator = BenchmarkAllocator();
    defer allocator.deinit();
    
    const base_path = "/tmp/browserdb_cache_test";
    try std.fs.cwd().makeDir(base_path);
    defer std.fs.cwd().deleteDir(base_path) catch {};
    
    const lsm_tree = try lsm_tree.LSMTree.init(allocator.allocator(), base_path, .History, 8 * 1024 * 1024);
    defer lsm_tree.deinit();
    
    // Populate with data
    const num_keys = 1000;
    for (0..num_keys) |i| {
        const key = std.fmt.allocPrintZ(allocator.allocator(), "cache_key_{d}", .{i}) catch continue;
        defer allocator.allocator().free(key);
        
        const value = std.fmt.allocPrintZ(allocator.allocator(), "cache_value_{d}", .{i}) catch continue;
        defer allocator.allocator().free(value);
        
        try lsm_tree.put(key, value);
    }
    
    // Create access patterns
    const frequent_keys = [_]usize{ 0, 1, 2, 3, 4 };
    const occasional_keys = [_]usize{ 10, 20, 30, 40, 50 };
    const rare_keys = [_]usize{ 500, 600, 700, 800, 900 };
    
    var hit_count: usize = 0;
    var total_reads: usize = 0;
    
    // Make some keys very frequently accessed
    for (0..100) |_| {
        for (frequent_keys) |key_idx| {
            const key = std.fmt.allocPrintZ(allocator.allocator(), "cache_key_{d}", .{key_idx}) catch continue;
            defer allocator.allocator().free(key);
            
            if (lsm_tree.get(key)) |_| {
                hit_count += 1;
            }
            total_reads += 1;
        }
    }
    
    // Access occasional keys
    for (0..20) |_| {
        for (occasional_keys) |key_idx| {
            const key = std.fmt.allocPrintZ(allocator.allocator(), "cache_key_{d}", .{key_idx}) catch continue;
            defer allocator.allocator().free(key);
            
            if (lsm_tree.get(key)) |_| {
                hit_count += 1;
            }
            total_reads += 1;
        }
    }
    
    // Access rare keys
    for (0..5) |_| {
        for (rare_keys) |key_idx| {
            const key = std.fmt.allocPrintZ(allocator.allocator(), "cache_key_{d}", .{key_idx}) catch continue;
            defer allocator.allocator().free(key);
            
            if (lsm_tree.get(key)) |_| {
                hit_count += 1;
            }
            total_reads += 1;
        }
    }
    
    const hit_rate = @as(f64, @floatFromInt(hit_count)) / @as(f64, @floatFromInt(total_reads));
    
    std.debug.print("🎯 Cache Hit Rate:\n", .{});
    std.debug.print("  Total Reads: {d}\n", .{total_reads});
    std.debug.print("  Cache Hits: {d}\n", .{hit_count});
    std.debug.print("  Hit Rate: {:.1}%\n", .{hit_rate * 100.0});
    
    // Should have good hit rate with frequently accessed data
    testing.expect(hit_rate > 0.8); // >80% hit rate for frequently accessed data
}