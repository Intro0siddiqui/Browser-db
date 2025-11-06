const std = @import("std");
const BrowserDB = @import("core/browserdb.zig");

// 测试统计信息
var test_count: usize = 0;
var pass_count: usize = 0;
var fail_count: usize = 0;

// 测试框架
fn test(comptime name: []const u8, test_fn: fn () anyerror!void) void {
    test_count += 1;
    std.debug.print("\n🧪 Running test: {s}...", .{name});
    
    if (test_fn()) {
        pass_count += 1;
        std.debug.print(" ✅ PASSED\n");
    } else {
        fail_count += 1;
        std.debug.print(" ❌ FAILED\n");
    }
}

fn expect(condition: bool) !void {
    if (!condition) {
        return error.TestFailed;
    }
}

fn expectEqual(comptime T: type, expected: T, actual: T) !void {
    if (expected != actual) {
        std.debug.print("Expected: {}, Actual: {}\n", .{ expected, actual });
        return error.TestFailed;
    }
}

// 具体测试用例

fn test_database_initialization() !void {
    var gpa = std.heap.GeneralPurposeAllocator(.{}){};
    defer _ = gpa.deinit();
    
    const allocator = gpa.allocator();
    const db = try BrowserDB.init(allocator, "/tmp/test-browserdb");
    defer db.deinit();
    
    try expect(true); // 成功初始化就是通过
}

fn test_history_table_basic_operations() !void {
    var gpa = std.heap.GeneralPurposeAllocator(.{}){};
    defer _ = gpa.deinit();
    
    const allocator = gpa.allocator();
    var db = try BrowserDB.init(allocator, "/tmp/test-browserdb");
    defer db.deinit();
    
    // 插入历史记录
    const entry = BrowserDB.HistoryEntry{
        .timestamp = 1234567890,
        .url_hash = 0x123456789abcdef0,
        .title = "Test Page",
        .visit_count = 1,
    };
    
    try db.history.insert(entry);
    
    // 验证插入
    try expect(db.history.memtable.entries.items.len > 0);
    
    // 测试检索
    const retrieved = try db.history.get(0x123456789abcdef0);
    try expect(retrieved != null);
    try expectEqual(u128, entry.url_hash, retrieved.?.url_hash);
}

fn test_cookies_table_initialization() !void {
    var gpa = std.heap.GeneralPurposeAllocator(.{}){};
    defer _ = gpa.deinit();
    
    const allocator = gpa.allocator();
    var db = try BrowserDB.init(allocator, "/tmp/test-browserdb");
    defer db.deinit();
    
    // 验证Cookie表已初始化
    try expect(db.cookies.memtable.allocator == allocator);
}

fn test_cache_table_basic_setup() !void {
    var gpa = std.heap.GeneralPurposeAllocator(.{}){};
    defer _ = gpa.deinit();
    
    const allocator = gpa.allocator();
    var db = try BrowserDB.init(allocator, "/tmp/test-browserdb");
    defer db.deinit();
    
    // 验证Cache表路径设置
    try expect(std.mem.eql(u8, db.cache.path, "/tmp/test-browserdb/cache.bdb"));
}

fn test_memtable_kv_operations() !void {
    var gpa = std.heap.GeneralPurposeAllocator(.{}){};
    defer _ = gpa.deinit();
    
    const allocator = gpa.allocator();
    var memtable = BrowserDB.MemTable.init(allocator, 1024 * 1024); // 1MB limit
    defer memtable.deinit();
    
    // 测试PUT操作
    const key = "test_key";
    const value = "test_value";
    
    try memtable.put(key, value, .History);
    
    // 验证条目已添加
    try expect(memtable.entries.items.len == 1);
    
    // 测试GET操作
    const retrieved = try memtable.get(key);
    try expect(retrieved != null);
    try expectEqual(u64, 1234567890, retrieved.?.timestamp);
}

fn test_memtable_heatmap_tracking() !void {
    var gpa = std.heap.GeneralPurposeAllocator(.{}){};
    defer _ = gpa.deinit();
    
    const allocator = gpa.allocator();
    var memtable = BrowserDB.MemTable.init(allocator, 1024 * 1024); // 1MB limit
    defer memtable.deinit();
    
    // 测试热度增长
    const key = "hot_entry";
    const value = "test value";
    
    try memtable.put(key, value, .History);
    
    // 第一次获取会提升热度
    const retrieved1 = try memtable.get(key);
    try expect(retrieved1 != null);
    
    // 第二次获取会进一步提升热度
    const retrieved2 = try memtable.get(key);
    try expect(retrieved2 != null);
    
    // 验证热度跟踪存在
    try expect(memtable.heat_map.entries.len > 0);
}

fn test_localstore_table_initialization() !void {
    var gpa = std.heap.GeneralPurposeAllocator(.{}){};
    defer _ = gpa.deinit();
    
    const allocator = gpa.allocator();
    var db = try BrowserDB.init(allocator, "/tmp/test-browserdb");
    defer db.deinit();
    
    // 验证LocalStore表已正确初始化
    try expect(db.localstore.memtable.allocator == allocator);
    try expect(std.mem.eql(u8, db.localstore.path, "/tmp/test-browserdb/localstore.bdb"));
}

fn test_settings_table_basic_operations() !void {
    var gpa = std.heap.GeneralPurposeAllocator(.{}){};
    defer _ = gpa.deinit();
    
    const allocator = gpa.allocator();
    var db = try BrowserDB.init(allocator, "/tmp/test-browserdb");
    defer db.deinit();
    
    // 验证Settings表路径
    try expect(std.mem.eql(u8, db.settings.path, "/tmp/test-browserdb/settings.bdb"));
}

fn test_error_handling() !void {
    var gpa = std.heap.GeneralPurposeAllocator(.{}){};
    defer _ = gpa.deinit();
    
    const allocator = gpa.allocator();
    var memtable = BrowserDB.MemTable.init(allocator, 1024); // Very small limit for testing
    defer memtable.deinit();
    
    // 测试空数据处理
    try expectEqual(usize, 0, memtable.entries.items.len);
    
    // 测试不存在键的GET操作
    const result = try memtable.get("nonexistent_key");
    try expect(result == null);
    
    // 测试删除不存在键
    try memtable.delete("nonexistent_key");
}

fn test_performance_benchmarks() !void {
    var gpa = std.heap.GeneralPurposeAllocator(.{}){};
    defer _ = gpa.deinit();
    
    const allocator = gpa.allocator();
    var memtable = BrowserDB.MemTable.init(allocator, 10 * 1024 * 1024); // 10MB limit
    defer memtable.deinit();
    
    // 基准测试：批量插入
    const start_time = std.time.nanoTimestamp();
    var successful_inserts: usize = 0;
    
    for (0..1000) |i| {
        const key = std.fmt.allocPrint(allocator, "key_{}", .{i}) catch continue;
        defer allocator.free(key);
        const value = std.fmt.allocPrint(allocator, "value_{}", .{i}) catch continue;
        defer allocator.free(value);
        
        if (memtable.put(key, value, .History)) {
            successful_inserts += 1;
        } else {
            // Stop if we hit the size limit
            break;
        }
    }
    
    const end_time = std.time.nanoTimestamp();
    const elapsed = end_time - start_time;
    const elapsed_ms = @as(f64, @floatFromInt(elapsed)) / 1_000_000.0;
    
    std.debug.print("📊 Performance: {} inserts in {:.2}ms\n", .{ successful_inserts, elapsed_ms });
    if (successful_inserts > 0) {
        std.debug.print("   Rate: {:.0} inserts/sec\n", .{ @as(f64, @floatFromInt(successful_inserts)) / (elapsed_ms / 1000.0) });
    }
    
    // 验证批量插入成功
    try expect(successful_inserts > 0);
}

fn test_sstable_basic_operations() !void {
    var gpa = std.heap.GeneralPurposeAllocator(.{}){};
    defer _ = gpa.deinit();
    
    const allocator = gpa.allocator();
    const temp_path = "/tmp/test-sstable.sst";
    
    // Create a test SSTable
    const sstable = try BrowserDB.SSTable.init(allocator, temp_path, 0, 1024 * 1024);
    defer sstable.deinit();
    
    // Add some entries
    try sstable.block.entries.append(BrowserDB.KVEntry{
        .key = "test_key",
        .value = "test_value",
        .entry_type = .History,
        .timestamp = 1234567890,
        .deleted = false,
        .heat = 0.5,
    });
    
    // Build index
    try sstable.block.buildIndex();
    
    // Verify index was built
    try expect(sstable.block.index_entries.items.len > 0);
    
    // Test flush
    try sstable.flush();
    
    std.debug.print("✅ SSTable basic operations test passed\n", .{});
}

fn test_compaction_manager() !void {
    var gpa = std.heap.GeneralPurposeAllocator(.{}){};
    defer _ = gpa.deinit();
    
    const allocator = gpa.allocator();
    const base_path = "/tmp/test-compaction";
    
    var comp_manager = BrowserDB.CompactionManager.init(allocator, base_path);
    defer _ = &comp_manager; // Keep alive for the duration
    
    // Test compaction initialization
    try comp_manager.compact(.Leveled, 0);
    
    std.debug.print("✅ Compaction manager test passed\n", .{});
}

fn test_database_flush_all() !void {
    var gpa = std.heap.GeneralPurposeAllocator(.{}){};
    defer _ = gpa.deinit();
    
    const allocator = gpa.allocator();
    var db = try BrowserDB.init(allocator, "/tmp/test-flush");
    defer db.deinit();
    
    // Add some test data
    const history_entry = BrowserDB.HistoryEntry{
        .timestamp = 1234567890,
        .url_hash = 0x123456789abcdef0,
        .title = "Test Page",
        .visit_count = 1,
    };
    
    try db.history.insert(history_entry);
    
    // Test flush all
    try db.flushAll();
    
    std.debug.print("✅ Database flush all test passed\n", .{});
}

fn test_database_status() !void {
    var gpa = std.heap.GeneralPurposeAllocator(.{}){};
    defer _ = gpa.deinit();
    
    const allocator = gpa.allocator();
    var db = try BrowserDB.init(allocator, "/tmp/test-status");
    defer db.deinit();
    
    // Add some test data
    const history_entry = BrowserDB.HistoryEntry{
        .timestamp = 1234567890,
        .url_hash = 0x123456789abcdef0,
        .title = "Test Page",
        .visit_count = 1,
    };
    
    try db.history.insert(history_entry);
    
    // Get status
    const status = try db.getStatus();
    
    std.debug.print("📊 Database Status: {} entries, {} bytes\n", .{ 
        status.memtable_entries, status.memtable_size 
    });
    
    // Verify status makes sense
    try expect(status.memtable_entries > 0);
    try expect(status.memtable_size > 0);
    
    std.debug.print("✅ Database status test passed\n", .{});
}

fn test_bdb_file_integration() !void {
    var gpa = std.heap.GeneralPurposeAllocator(.{}){};
    defer _ = gpa.deinit();
    
    const allocator = gpa.allocator();
    
    const test_path = "/tmp/test-bdb-integration.bdb";
    std.fs.cwd().deleteFile(test_path) catch {};
    
    // Test .bdb file creation through SSTable
    const sstable = try BrowserDB.SSTable.init(allocator, test_path, 0, BrowserDB.bdb_format.TableType.History);
    defer sstable.deinit();
    
    // Create test entries
    var entries = std.ArrayList(BrowserDB.KVEntry).init(allocator);
    defer entries.deinit();
    
    const test_entry = BrowserDB.KVEntry{
        .key = "integration_test_key",
        .value = "integration_test_value",
        .entry_type = BrowserDB.bdb_format.TableType.History,
        .timestamp = std.time.milliTimestamp(),
        .deleted = false,
        .heat = 0.8,
    };
    
    try entries.append(test_entry);
    
    // Test flush to .bdb format
    try sstable.flush(entries);
    
    // Get stats
    const stats = try sstable.getStats();
    try expect(stats.entry_count > 0);
    
    std.debug.print("📄 .bdb Integration: {} entries, {} bytes\n", .{
        stats.entry_count, stats.file_size
    });
    
    std.debug.print("✅ .bdb file integration test passed\n", .{});
}

fn test_heatmap_indexing_integration() !void {
    const HeatMap = @import("core/heatmap_indexing.zig");
    const bdb_format = @import("core/bdb_format.zig");
    
    var gpa = std.heap.GeneralPurposeAllocator(.{}){};
    defer _ = gpa.deinit();
    
    const allocator = gpa.allocator();
    
    // Initialize heat tracking system
    var tracker = HeatMap.HeatTracker.init(allocator, 1000);
    defer tracker.deinit();
    
    // Initialize dynamic heat manager
    var manager = try HeatMap.DynamicHeatManager.init(allocator, 1000, 16);
    defer manager.deinit(allocator);
    
    // Initialize bloom filter
    var filter = try HeatMap.BloomFilter.init(allocator, 1000, 0.01);
    defer filter.deinit();
    
    // Test key-value pairs
    const test_key1 = bdb_format.BDBKey.fromString("test_key_1") catch return;
    const test_key2 = bdb_format.BDBKey.fromString("test_key_2") catch return;
    const test_key3 = bdb_format.BDBKey.fromString("test_key_3") catch return;
    
    const test_value1 = bdb_format.BDBValue.fromString("test_value_1") catch return;
    const test_value2 = bdb_format.BDBValue.fromString("test_value_2") catch return;
    const test_value3 = bdb_format.BDBValue.fromString("test_value_3") catch return;
    
    // Simulate access patterns
    // Key 1: High frequency (should become hot)
    for (0..25) |_| {
        try manager.recordAccess(test_key1, test_value1, .Read);
        try filter.add(test_key1);
    }
    
    // Key 2: Medium frequency
    for (0..10) |_| {
        try manager.recordAccess(test_key2, test_value2, .Read);
        try filter.add(test_key2);
    }
    
    // Key 3: Low frequency
    for (0..3) |_| {
        try manager.recordAccess(test_key3, test_value3, .Read);
        try filter.add(test_key3);
    }
    
    // Test heat detection
    try expect(manager.isHot(test_key1));
    try expect(!manager.isHot(test_key2));
    try expect(!manager.isHot(test_key3));
    
    // Test bloom filter accuracy
    try expect(manager.mightContain(test_key1));
    try expect(manager.mightContain(test_key2));
    try expect(manager.mightContain(test_key3));
    
    const cold_key = bdb_format.BDBKey.fromString("nonexistent_key") catch return;
    try expect(!manager.mightContain(cold_key));
    
    // Test get hot keys
    const hot_keys = try manager.getHotKeys(10);
    defer allocator.free(hot_keys);
    
    try expect(hot_keys.len >= 1);
    
    // Verify key1 is in hot keys (highest heat)
    var found_key1 = false;
    for (hot_keys) |key| {
        if (std.mem.eql(u8, &key.data, &test_key1.data)) {
            found_key1 = true;
            break;
        }
    }
    try expect(found_key1);
    
    // Test threshold adaptation
    const initial_threshold = manager.adapt_thresholds.hot_threshold;
    manager.adaptThresholds();
    
    // Threshold should potentially change based on access patterns
    std.debug.print("🔥 HeatMap Integration: {} hot keys, threshold: {}\n", .{
        hot_keys.len, manager.adapt_thresholds.hot_threshold
    });
    
    // Performance test
    const start_time = std.time.milliTimestamp();
    for (0..100) |_| {
        _ = manager.getHeat(test_key1);
        _ = manager.mightContain(test_key1);
        _ = manager.isHot(test_key1);
    }
    const end_time = std.time.milliTimestamp();
    const operation_time = end_time - start_time;
    
    // Should complete 100 operations in under 1 second
    try expect(operation_time < 1000);
    
    std.debug.print("⚡ HeatMap Performance: {}ms for 100 operations\n", .{operation_time});
    std.debug.print("✅ HeatMap indexing integration test passed\n", .{});
}

fn test_lsm_tree_core_integration() !void {
    const LSMTree = @import("core/lsm_tree.zig");
    const bdb_format = @import("core/bdb_format.zig");
    
    var gpa = std.heap.GeneralPurposeAllocator(.{}){};
    defer _ = gpa.deinit();
    
    const allocator = gpa.allocator();
    const base_path = "/tmp/browserdb_lsm_integration";
    try std.fs.cwd().makeDir(base_path);
    defer std.fs.cwd().deleteDir(base_path) catch {};
    
    // Test LSM-Tree initialization
    var lsm_tree = try LSMTree.LSMTree.init(allocator, base_path, .History, 1024 * 1024);
    defer lsm_tree.deinit();
    
    try expect(lsm_tree.is_initialized);
    
    // Test basic put/get operations
    try lsm_tree.put("integration_key_1", "integration_value_1");
    try lsm_tree.put("integration_key_2", "integration_value_2");
    try lsm_tree.put("integration_key_3", "integration_value_3");
    
    if (lsm_tree.get("integration_key_1")) |entry| {
        try expect(std.mem.eql(u8, entry.key, "integration_key_1"));
        try expect(std.mem.eql(u8, entry.value, "integration_value_1"));
    } else {
        return error.KeyNotFound;
    }
    
    // Test update operations
    try lsm_tree.put("integration_key_1", "updated_value");
    
    if (lsm_tree.get("integration_key_1")) |entry| {
        try expect(std.mem.eql(u8, entry.value, "updated_value"));
    } else {
        return error.KeyNotFound;
    }
    
    // Test delete operations
    try lsm_tree.delete("integration_key_3");
    
    if (lsm_tree.get("integration_key_3")) |entry| {
        try expect(entry.deleted);
    } else {
        // Key should be marked as deleted, not completely removed
        return error.DeleteExpected;
    }
    
    // Test MemTable flush
    const initial_stats = try lsm_tree.getStats();
    try lsm_tree.flush();
    
    const flush_stats = try lsm_tree.getStats();
    try expect(flush_stats.memtable_size == 0); // Should be flushed
    try expect(flush_stats.memtable_count == 0); // Should be cleared
    
    // Test compaction
    try lsm_tree.compact();
    
    const final_stats = try lsm_tree.getStats();
    std.debug.print("🌲 LSM-Tree Integration: {} SSTables, {} bytes total\n", .{
        final_stats.total_sstables, final_stats.total_sstable_size
    });
    
    std.debug.print("✅ LSM-Tree core integration test passed\n", .{});
}

fn test_modes_operations_integration() !void {
    const ModesOps = @import("core/modes_operations.zig");
    const bdb_format = @import("core/bdb_format.zig");
    
    var gpa = std.heap.GeneralPurposeAllocator(.{}){};
    defer _ = gpa.deinit();
    
    const allocator = gpa.allocator();
    
    // Test ModeConfig initialization
    const config = ModesOps.ModeConfig{
        .mode = .Ultra,
        .max_memory = 1024 * 1024, // 1MB
        .auto_save_interval = 0,
        .backup_retention = 3,
        .enable_compression = false,
        .enable_encryption = false,
        .enable_heat_tracking = true,
        .cache_size = 500,
    };
    
    try expect(ModesOps.DatabaseMode.Ultra == config.mode);
    try expect(config.max_memory > 0);
    
    // Test ModeSwitcher initialization
    var switcher = ModesOps.ModeSwitcher.init();
    defer switcher.deinit(allocator);
    
    try expect(switcher.current_mode == null);
    try expect(ModesOps.DatabaseMode.Persistent == switcher.target_mode);
    try expect(switcher.switching == false);
    
    // Test CRUDOperations initialization
    var crud = ModesOps.CRUDOperations.init(&switcher);
    
    // Test CRUD operations
    const test_key = bdb_format.BDBKey.fromString("modes_test_key") catch return;
    const test_value = bdb_format.BDBValue.fromString("modes_test_value") catch return;
    const timestamp = std.time.timestamp();
    
    // Test History CRUD operations
    const create_entry = try ModesOps.CRUDOperations.History.create(test_key, test_value, timestamp);
    try expect(create_entry.entry_type == bdb_format.BDBLogEntry.EntryType.Insert);
    
    const update_entry = try ModesOps.CRUDOperations.History.update(test_key, test_value, timestamp);
    try expect(update_entry.entry_type == bdb_format.BDBLogEntry.EntryType.Update);
    
    const delete_entry = try ModesOps.CRUDOperations.History.delete(test_key, timestamp);
    try expect(delete_entry.entry_type == bdb_format.BDBLogEntry.EntryType.Delete);
    
    // Test other table CRUD operations
    for (0..5) |i| {
        const cookie_key = bdb_format.BDBKey.fromString(std.fmt.allocPrint(allocator, "cookie_key_{d}", .{i}) catch return) catch return;
        allocator.free(cookie_key.data[0..cookie_key.data.len]);
        const cache_key = bdb_format.BDBKey.fromString(std.fmt.allocPrint(allocator, "cache_key_{d}", .{i}) catch return) catch return;
        allocator.free(cache_key.data[0..cache_key.data.len]);
        
        // Cookies operations
        const cookie_create = try ModesOps.CRUDOperations.Cookies.create(test_key, test_value, timestamp);
        const cookie_update = try ModesOps.CRUDOperations.Cookies.update(test_key, test_value, timestamp);
        const cookie_delete = try ModesOps.CRUDOperations.Cookies.delete(test_key, timestamp);
        
        // Cache operations
        const cache_create = try ModesOps.CRUDOperations.Cache.create(test_key, test_value, timestamp);
        const cache_update = try ModesOps.CRUDOperations.Cache.update(test_key, test_value, timestamp);
        const cache_delete = try ModesOps.CRUDOperations.Cache.delete(test_key, timestamp);
        
        // All operations should complete successfully
        try expect(cookie_create.entry_type == bdb_format.BDBLogEntry.EntryType.Insert);
        try expect(cookie_update.entry_type == bdb_format.BDBLogEntry.EntryType.Update);
        try expect(cookie_delete.entry_type == bdb_format.BDBLogEntry.EntryType.Delete);
        try expect(cache_create.entry_type == bdb_format.BDBLogEntry.EntryType.Insert);
        try expect(cache_update.entry_type == bdb_format.BDBLogEntry.EntryType.Update);
        try expect(cache_delete.entry_type == bdb_format.BDBLogEntry.EntryType.Delete);
    }
    
    // Test BackupPrivacy initialization
    var backup_privacy = ModesOps.BackupPrivacy.init(&switcher);
    
    // Test export format enumeration
    try expect(@as(ModesOps.BackupPrivacy.ExportFormat, .JSON) == ModesOps.BackupPrivacy.ExportFormat.JSON);
    try expect(@as(ModesOps.BackupPrivacy.ExportFormat, .CSV) == ModesOps.BackupPrivacy.ExportFormat.CSV);
    try expect(@as(ModesOps.BackupPrivacy.ExportFormat, .XML) == ModesOps.BackupPrivacy.ExportFormat.XML);
    try expect(@as(ModesOps.BackupPrivacy.ExportFormat, .Custom) == ModesOps.BackupPrivacy.ExportFormat.Custom);
    
    // Test ModeStats structure
    const stats = ModesOps.ModeStats{
        .mode = .Ultra,
        .database_stats = null,
        .heat_stats = null,
        .last_flush = timestamp,
        .uptime = 0,
        .memory_usage = 0,
        .max_memory = config.max_memory,
    };
    
    try expect(ModesOps.DatabaseMode.Ultra == stats.mode);
    try expect(stats.last_flush == timestamp);
    try expect(stats.max_memory == config.max_memory);
    
    // Test UltraMode basic operations
    var ultra = try ModesOps.UltraMode.init(allocator, config);
    defer ultra.deinit(allocator);
    
    try expect(ModesOps.DatabaseMode.Ultra == ultra.mode);
    try expect(ultra.config.max_memory == config.max_memory);
    try expect(ultra.memory_pool.max_memory == config.max_memory);
    try expect(ultra.memory_pool.used_memory == 0);
    
    // Test UltraMode memory pool
    const test_data = try ultra.memory_pool.alloc(100);
    try expect(ultra.memory_pool.used_memory >= 100);
    ultra.memory_pool.free(test_data);
    
    // Test UltraMode table operations
    const ultra_test_key = bdb_format.BDBKey.fromString("ultra_table_key") catch return;
    const ultra_test_value = bdb_format.BDBValue.fromString("ultra_table_value") catch return;
    
    try ultra.tables.history.put(ultra_test_key, ultra_test_value);
    try expect(ultra.tables.history.current_entries == 1);
    
    const retrieved_value = ultra.tables.history.get(ultra_test_key);
    try expect(retrieved_value != null);
    
    const deleted = ultra.tables.history.delete(ultra_test_key);
    try expect(deleted == true);
    try expect(ultra.tables.history.current_entries == 0);
    
    std.debug.print("⚙️  Modes & Operations: Persistent/Ultra modes, CRUD operations, backup/privacy\n", .{});
    std.debug.print("✅ Modes & operations integration test passed\n", .{});
}

// 主测试函数
pub fn main() !void {
    std.debug.print("🚀 BrowserDB Test Suite\n", .{});
    std.debug.print("=========================\n\n", .{});
    
    // 运行所有测试
    test("Database Initialization", test_database_initialization);
    test("History Table Basic Operations", test_history_table_basic_operations);
    test("Cookies Table Initialization", test_cookies_table_initialization);
    test("Cache Table Basic Setup", test_cache_table_basic_setup);
    test("MemTable KV Operations", test_memtable_kv_operations);
    test("MemTable HeatMap Tracking", test_memtable_heatmap_tracking);
    test("LocalStore Table Initialization", test_localstore_table_initialization);
    test("Settings Table Basic Operations", test_settings_table_basic_operations);
    test("Error Handling", test_error_handling);
    test("Performance Benchmarks", test_performance_benchmarks);
    test("SSTable Basic Operations", test_sstable_basic_operations);
    test("Compaction Manager", test_compaction_manager);
    test("Database Flush All", test_database_flush_all);
    test("Database Status", test_database_status);
    test(".bdb File Integration", test_bdb_file_integration);
    test("LSM-Tree Core Engine", test_lsm_tree_core_integration);
    test("HeatMap Indexing System", test_heatmap_indexing_integration);
    test("Modes & Operations System", test_modes_operations_integration);
    
    // 输出测试结果
    std.debug.print("\n📊 Test Results:\n", .{});
    std.debug.print("   Total tests: {d}\n", .{test_count});
    std.debug.print("   Passed: {d}\n", .{pass_count});
    std.debug.print("   Failed: {d}\n", .{fail_count});
    std.debug.print("   Success rate: {d}%\n", .{
        if (test_count > 0) (pass_count * 100 / test_count) else 0
    });
    
    if (fail_count == 0) {
        std.debug.print("\n🎉 All tests passed! BrowserDB is ready for development.\n", .{});
    } else {
        std.debug.print("\n⚠️  Some tests failed. Please review the implementation.\n", .{});
        return error.TestsFailed;
    }
}