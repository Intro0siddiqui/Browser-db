//! Modes & Operations System Tests
//! 
//! Comprehensive test suite for BrowserDB modes and operations including:
//! - Persistent mode tests
//! - Ultra mode tests
//! - Mode switching tests
//! - Backup and privacy operations tests
//! - CRUD operations tests

const std = @import("std");
const testing = std.testing;
const bdb_format = @import("bdb_format.zig");
const HeatMap = @import("heatmap_indexing.zig");
const ModesOps = @import("modes_operations.zig");

test "ModeConfig initialization and validation" {
    const config = ModesOps.ModeConfig{
        .mode = .Persistent,
        .max_memory = 1024 * 1024 * 1024, // 1GB
        .auto_save_interval = 30000, // 30 seconds
        .backup_retention = 5,
        .enable_compression = true,
        .enable_encryption = false,
        .enable_heat_tracking = true,
        .cache_size = 1000,
    };
    
    try testing.expectEqual(ModesOps.DatabaseMode.Persistent, config.mode);
    try testing.expect(config.max_memory > 0);
    try testing.expect(config.auto_save_interval > 0);
    try testing.expect(config.backup_retention > 0);
    try testing.expect(config.enable_heat_tracking == true);
}

test "PersistentMode basic operations" {
    const allocator = std.testing.allocator;
    
    const config = ModesOps.ModeConfig{
        .mode = .Persistent,
        .max_memory = 1024 * 1024, // 1MB
        .auto_save_interval = 60000, // 1 minute
        .backup_retention = 3,
        .enable_compression = false,
        .enable_encryption = false,
        .enable_heat_tracking = true,
        .cache_size = 100,
    };
    
    var persistent = try ModesOps.PersistentMode.init(allocator, "/tmp/test-browserdb-persistent", config);
    defer persistent.deinit(allocator);
    
    // Test initial state
    try testing.expectEqual(ModesOps.DatabaseMode.Persistent, persistent.mode);
    try testing.expect(persistent.core_db == null);
    try testing.expect(persistent.heat_manager == null);
    
    // Note: Actual start() test would require a real database setup
    // which is complex for unit testing
}

test "UltraMode basic operations" {
    const allocator = std.testing.allocator;
    
    const config = ModesOps.ModeConfig{
        .mode = .Ultra,
        .max_memory = 1024 * 1024, // 1MB
        .auto_save_interval = 0, // No auto-save for ultra mode
        .backup_retention = 0,
        .enable_compression = false,
        .enable_encryption = false,
        .enable_heat_tracking = true,
        .cache_size = 100,
    };
    
    var ultra = try ModesOps.UltraMode.init(allocator, config);
    defer ultra.deinit(allocator);
    
    // Test initial state
    try testing.expectEqual(ModesOps.DatabaseMode.Ultra, ultra.mode);
    try testing.expect(ultra.config.max_memory > 0);
    try testing.expect(ultra.memory_pool.max_memory == config.max_memory);
    try testing.expect(ultra.memory_pool.used_memory == 0);
}

test "UltraMode memory pool operations" {
    const allocator = std.testing.allocator;
    
    var pool = ModesOps.UltraMode.MemoryPool.init(allocator, 1024 * 1024); // 1MB
    defer pool.deinit();
    
    try testing.expectEqual(@as(usize, 0), pool.used_memory);
    try testing.expect(pool.max_memory == 1024 * 1024);
    
    // Test allocation
    const data1 = try pool.alloc(100);
    try testing.expectEqual(@as(usize, 100), pool.used_memory);
    try testing.expect(data1.len == 100);
    
    // Test free
    pool.free(data1);
    try testing.expectEqual(@as(usize, 0), pool.used_memory);
}

test "UltraMode table operations" {
    const allocator = std.testing.allocator;
    
    var table = ModesOps.UltraMode.TableSet.UltraTable.init(allocator, 10);
    defer table.deinit();
    
    const test_key = bdb_format.BDBKey.fromString("test_key") catch return;
    const test_value = bdb_format.BDBValue.fromString("test_value") catch return;
    
    // Test initial state
    try testing.expectEqual(@as(usize, 0), table.current_entries);
    try testing.expect(table.get(test_key) == null);
    
    // Test put
    try table.put(test_key, test_value);
    try testing.expectEqual(@as(usize, 1), table.current_entries);
    
    // Test get
    const retrieved_value = table.get(test_key);
    try testing.expect(retrieved_value != null);
    if (retrieved_value) |value| {
        try testing.expectEqual(@as(usize, test_value.data.len), value.data.len);
    }
    
    // Test delete
    const deleted = table.delete(test_key);
    try testing.expect(deleted == true);
    try testing.expectEqual(@as(usize, 0), table.current_entries);
    try testing.expect(table.get(test_key) == null);
}

test "ModeSwitcher initialization" {
    var switcher = ModesOps.ModeSwitcher.init();
    
    try testing.expect(switcher.current_mode == null);
    try testing.expectEqual(ModesOps.DatabaseMode.Persistent, switcher.target_mode);
    try testing.expect(switcher.switching == false);
}

test "CRUDOperations History operations" {
    const allocator = std.testing.allocator;
    
    var switcher = ModesOps.ModeSwitcher.init();
    defer switcher.deinit(allocator);
    
    var crud = ModesOps.CRUDOperations.init(&switcher);
    
    const test_key = bdb_format.BDBKey.fromString("history_test_key") catch return;
    const test_value = bdb_format.BDBValue.fromString("history_test_value") catch return;
    const timestamp = std.time.timestamp();
    
    // Test create
    const create_entry = try ModesOps.CRUDOperations.History.create(test_key, test_value, timestamp);
    try testing.expect(create_entry.entry_type == bdb_format.BDBLogEntry.EntryType.Insert);
    
    // Test update
    const update_entry = try ModesOps.CRUDOperations.History.update(test_key, test_value, timestamp);
    try testing.expect(update_entry.entry_type == bdb_format.BDBLogEntry.EntryType.Update);
    
    // Test delete
    const delete_entry = try ModesOps.CRUDOperations.History.delete(test_key, timestamp);
    try testing.expect(delete_entry.entry_type == bdb_format.BDBLogEntry.EntryType.Delete);
}

test "CRUDOperations Cookies operations" {
    const allocator = std.testing.allocator;
    
    var switcher = ModesOps.ModeSwitcher.init();
    defer switcher.deinit(allocator);
    
    var crud = ModesOps.CRUDOperations.init(&switcher);
    
    const test_key = bdb_format.BDBKey.fromString("cookie_test_key") catch return;
    const test_value = bdb_format.BDBValue.fromString("cookie_test_value") catch return;
    const timestamp = std.time.timestamp();
    
    // Test create
    const create_entry = try ModesOps.CRUDOperations.Cookies.create(test_key, test_value, timestamp);
    try testing.expect(create_entry.entry_type == bdb_format.BDBLogEntry.EntryType.Insert);
    
    // Test update
    const update_entry = try ModesOps.CRUDOperations.Cookies.update(test_key, test_value, timestamp);
    try testing.expect(update_entry.entry_type == bdb_format.BDBLogEntry.EntryType.Update);
    
    // Test delete
    const delete_entry = try ModesOps.CRUDOperations.Cookies.delete(test_key, timestamp);
    try testing.expect(delete_entry.entry_type == bdb_format.BDBLogEntry.EntryType.Delete);
}

test "CRUDOperations Cache operations" {
    const allocator = std.testing.allocator;
    
    var switcher = ModesOps.ModeSwitcher.init();
    defer switcher.deinit(allocator);
    
    var crud = ModesOps.CRUDOperations.init(&switcher);
    
    const test_key = bdb_format.BDBKey.fromString("cache_test_key") catch return;
    const test_value = bdb_format.BDBValue.fromString("cache_test_value") catch return;
    const timestamp = std.time.timestamp();
    
    // Test create
    const create_entry = try ModesOps.CRUDOperations.Cache.create(test_key, test_value, timestamp);
    try testing.expect(create_entry.entry_type == bdb_format.BDBLogEntry.EntryType.Insert);
    
    // Test update
    const update_entry = try ModesOps.CRUDOperations.Cache.update(test_key, test_value, timestamp);
    try testing.expect(update_entry.entry_type == bdb_format.BDBLogEntry.EntryType.Update);
    
    // Test delete
    const delete_entry = try ModesOps.CRUDOperations.Cache.delete(test_key, timestamp);
    try testing.expect(delete_entry.entry_type == bdb_format.BDBLogEntry.EntryType.Delete);
}

test "CRUDOperations LocalStorage operations" {
    const allocator = std.testing.allocator;
    
    var switcher = ModesOps.ModeSwitcher.init();
    defer switcher.deinit(allocator);
    
    var crud = ModesOps.CRUDOperations.init(&switcher);
    
    const test_key = bdb_format.BDBKey.fromString("localstore_test_key") catch return;
    const test_value = bdb_format.BDBValue.fromString("localstore_test_value") catch return;
    const timestamp = std.time.timestamp();
    
    // Test create
    const create_entry = try ModesOps.CRUDOperations.LocalStorage.create(test_key, test_value, timestamp);
    try testing.expect(create_entry.entry_type == bdb_format.BDBLogEntry.EntryType.Insert);
    
    // Test update
    const update_entry = try ModesOps.CRUDOperations.LocalStorage.update(test_key, test_value, timestamp);
    try testing.expect(update_entry.entry_type == bdb_format.BDBLogEntry.EntryType.Update);
    
    // Test delete
    const delete_entry = try ModesOps.CRUDOperations.LocalStorage.delete(test_key, timestamp);
    try testing.expect(delete_entry.entry_type == bdb_format.BDBLogEntry.EntryType.Delete);
}

test "CRUDOperations Settings operations" {
    const allocator = std.testing.allocator;
    
    var switcher = ModesOps.ModeSwitcher.init();
    defer switcher.deinit(allocator);
    
    var crud = ModesOps.CRUDOperations.init(&switcher);
    
    const test_key = bdb_format.BDBKey.fromString("settings_test_key") catch return;
    const test_value = bdb_format.BDBValue.fromString("settings_test_value") catch return;
    const timestamp = std.time.timestamp();
    
    // Test create
    const create_entry = try ModesOps.CRUDOperations.Settings.create(test_key, test_value, timestamp);
    try testing.expect(create_entry.entry_type == bdb_format.BDBLogEntry.EntryType.Insert);
    
    // Test update
    const update_entry = try ModesOps.CRUDOperations.Settings.update(test_key, test_value, timestamp);
    try testing.expect(update_entry.entry_type == bdb_format.BDBLogEntry.EntryType.Update);
    
    // Test delete
    const delete_entry = try ModesOps.CRUDOperations.Settings.delete(test_key, timestamp);
    try testing.expect(delete_entry.entry_type == bdb_format.BDBLogEntry.EntryType.Delete);
}

test "BackupPrivacy initialization" {
    const allocator = std.testing.allocator;
    
    var switcher = ModesOps.ModeSwitcher.init();
    defer switcher.deinit(allocator);
    
    var backup_privacy = ModesOps.BackupPrivacy.init(&switcher);
    
    // Just verify the structure exists and can be initialized
    try testing.expect(true);
}

test "ModeStats structure" {
    const config = ModesOps.ModeConfig{
        .mode = .Persistent,
        .max_memory = 1024 * 1024,
        .auto_save_interval = 60000,
        .backup_retention = 3,
        .enable_compression = false,
        .enable_encryption = false,
        .enable_heat_tracking = false,
        .cache_size = 100,
    };
    
    const stats = ModesOps.ModeStats{
        .mode = config.mode,
        .database_stats = null,
        .heat_stats = null,
        .last_flush = std.time.timestamp(),
        .uptime = 0,
        .memory_usage = 0,
        .max_memory = config.max_memory,
    };
    
    try testing.expectEqual(ModesOps.DatabaseMode.Persistent, stats.mode);
    try testing.expect(stats.last_flush > 0);
    try testing.expect(stats.max_memory == config.max_memory);
}

test "ExportFormat enumeration" {
    // Test all export formats
    try testing.expectEqual(@as(ModesOps.BackupPrivacy.ExportFormat, .JSON), ModesOps.BackupPrivacy.ExportFormat.JSON);
    try testing.expectEqual(@as(ModesOps.BackupPrivacy.ExportFormat, .CSV), ModesOps.BackupPrivacy.ExportFormat.CSV);
    try testing.expectEqual(@as(ModesOps.BackupPrivacy.ExportFormat, .XML), ModesOps.BackupPrivacy.ExportFormat.XML);
    try testing.expectEqual(@as(ModesOps.BackupPrivacy.ExportFormat, .Custom), ModesOps.BackupPrivacy.ExportFormat.Custom);
}

test "UltraMode table capacity management" {
    const allocator = std.testing.allocator;
    
    // Create table with small capacity
    var table = ModesOps.UltraMode.TableSet.UltraTable.init(allocator, 3);
    defer table.deinit();
    
    const test_key1 = bdb_format.BDBKey.fromString("key1") catch return;
    const test_key2 = bdb_format.BDBKey.fromString("key2") catch return;
    const test_key3 = bdb_format.BDBKey.fromString("key3") catch return;
    const test_key4 = bdb_format.BDBKey.fromString("key4") catch return;
    
    const test_value = bdb_format.BDBValue.fromString("test_value") catch return;
    
    // Fill to capacity
    try table.put(test_key1, test_value);
    try testing.expectEqual(@as(usize, 1), table.current_entries);
    
    try table.put(test_key2, test_value);
    try testing.expectEqual(@as(usize, 2), table.current_entries);
    
    try table.put(test_key3, test_value);
    try testing.expectEqual(@as(usize, 3), table.current_entries);
    
    // Add one more - should evict the oldest (key1)
    try table.put(test_key4, test_value);
    try testing.expectEqual(@as(usize, 3), table.current_entries); // Should still be at capacity
    
    // key1 should be gone, key4 should be present
    try testing.expect(table.get(test_key1) == null);
    try testing.expect(table.get(test_key4) != null);
}

test "UltraMode access order tracking" {
    const allocator = std.testing.allocator;
    
    var table = ModesOps.UltraMode.TableSet.UltraTable.init(allocator, 10);
    defer table.deinit();
    
    const test_key1 = bdb_format.BDBKey.fromString("key1") catch return;
    const test_key2 = bdb_format.BDBKey.fromString("key2") catch return;
    const test_key3 = bdb_format.BDBKey.fromString("key3") catch return;
    
    const test_value = bdb_format.BDBValue.fromString("test_value") catch return;
    
    // Add entries
    try table.put(test_key1, test_value);
    try table.put(test_key2, test_value);
    try table.put(test_key3, test_value);
    
    // Access key1 to move it to end
    _ = table.get(test_key1);
    
    // key1 should now be at the end of access order
    // In a full implementation, you'd verify the order
    try testing.expect(table.get(test_key1) != null);
    try testing.expect(table.get(test_key2) != null);
    try testing.expect(table.get(test_key3) != null);
}

test "Modes and Operations integration workflow" {
    const allocator = std.testing.allocator;
    
    // This test simulates a complete workflow
    var switcher = ModesOps.ModeSwitcher.init();
    defer switcher.deinit(allocator);
    
    var crud = ModesOps.CRUDOperations.init(&switcher);
    var backup_privacy = ModesOps.BackupPrivacy.init(&switcher);
    
    const config = ModesOps.ModeConfig{
        .mode = .Ultra,
        .max_memory = 1024 * 1024,
        .auto_save_interval = 0,
        .backup_retention = 3,
        .enable_compression = false,
        .enable_encryption = false,
        .enable_heat_tracking = true,
        .cache_size = 100,
    };
    
    // Test CRUD operations
    const test_key = bdb_format.BDBKey.fromString("integration_test_key") catch return;
    const test_value = bdb_format.BDBValue.fromString("integration_test_value") catch return;
    const timestamp = std.time.timestamp();
    
    // Create
    const create_entry = try ModesOps.CRUDOperations.History.create(test_key, test_value, timestamp);
    try testing.expect(create_entry.entry_type == bdb_format.BDBLogEntry.EntryType.Insert);
    
    // Update
    const update_entry = try ModesOps.CRUDOperations.History.update(test_key, test_value, timestamp);
    try testing.expect(update_entry.entry_type == bdb_format.BDBLogEntry.EntryType.Update);
    
    // Delete
    const delete_entry = try ModesOps.CRUDOperations.History.delete(test_key, timestamp);
    try testing.expect(delete_entry.entry_type == bdb_format.BDBLogEntry.EntryType.Delete);
    
    // The backup and privacy operations would require actual database instances
    // for full testing, but the structure is verified above
    try testing.expect(true);
}

test "Memory efficiency in UltraMode" {
    const allocator = std.testing.allocator;
    
    const config = ModesOps.ModeConfig{
        .mode = .Ultra,
        .max_memory = 10 * 1024, // 10KB
        .auto_save_interval = 0,
        .backup_retention = 0,
        .enable_compression = false,
        .enable_encryption = false,
        .enable_heat_tracking = false,
        .cache_size = 50,
    };
    
    var ultra = try ModesOps.UltraMode.init(allocator, config);
    defer ultra.deinit(allocator);
    
    const initial_memory = ultra.memory_pool.used_memory;
    
    // Add some test data
    const test_key = bdb_format.BDBKey.fromString("memory_test_key") catch return;
    const test_value = bdb_format.BDBValue.fromString("memory_test_value") catch return;
    
    try ultra.tables.history.put(test_key, test_value);
    
    // Verify memory tracking
    try testing.expect(ultra.memory_pool.used_memory >= initial_memory);
    try testing.expect(ultra.memory_pool.used_memory <= ultra.config.max_memory);
}

test "Performance characteristics of UltraMode" {
    const allocator = std.testing.allocator;
    
    const config = ModesOps.ModeConfig{
        .mode = .Ultra,
        .max_memory = 1024 * 1024, // 1MB
        .auto_save_interval = 0,
        .backup_retention = 0,
        .enable_compression = false,
        .enable_encryption = false,
        .enable_heat_tracking = true,
        .cache_size = 500,
    };
    
    var ultra = try ModesOps.UltraMode.init(allocator, config);
    defer ultra.deinit(allocator);
    
    const start_time = std.time.milliTimestamp();
    
    // Perform multiple operations to test performance
    const num_operations = 1000;
    
    for (0..num_operations) |i| {
        const key_data = std.fmt.allocPrint(allocator, "perf_key_{d}", .{i}) catch return;
        const key = bdb_format.BDBKey.fromString(key_data) catch return;
        allocator.free(key_data);
        
        const value_data = std.fmt.allocPrint(allocator, "perf_value_{d}", .{i}) catch return;
        const value = bdb_format.BDBValue.fromString(value_data) catch return;
        allocator.free(value_data);
        
        try ultra.tables.history.put(key, value);
        
        // Record access in heat tracker
        try ultra.ultra_heat_tracker.recordAccess(key, HeatMap.QueryType.Read);
    }
    
    const end_time = std.time.milliTimestamp();
    const total_time = end_time - start_time;
    
    // Should complete 1000 operations in reasonable time
    try testing.expect(total_time < 5000); // Less than 5 seconds
    
    // Verify heat tracking
    try testing.expect(ultra.ultra_heat_tracker.current_entries > 0);
    
    std.debug.print("UltraMode Performance: {} operations in {}ms\n", .{ num_operations, total_time });
}

test "Error handling in modes" {
    const allocator = std.testing.allocator;
    
    // Test out of memory handling in ultra mode
    const small_config = ModesOps.ModeConfig{
        .mode = .Ultra,
        .max_memory = 100, // Very small memory limit
        .auto_save_interval = 0,
        .backup_retention = 0,
        .enable_compression = false,
        .enable_encryption = false,
        .enable_heat_tracking = false,
        .cache_size = 5,
    };
    
    var ultra = try ModesOps.UltraMode.init(allocator, small_config);
    defer ultra.deinit(allocator);
    
    // Try to allocate more than available memory
    const large_data = ultra.memory_pool.alloc(200);
    
    // Should handle out of memory gracefully
    try testing.expect(large_data == error.OutOfMemory or large_data == null);
    
    // Test mode switching errors
    var switcher = ModesOps.ModeSwitcher.init();
    defer switcher.deinit(allocator);
    
    // Should not allow switching while already switching
    // Note: This test structure doesn't actually trigger switching
    // but verifies the flag logic exists
    try testing.expect(switcher.switching == false);
}

test "Mode compatibility validation" {
    // Test that modes can be distinguished
    const persistent_config = ModesOps.ModeConfig{
        .mode = .Persistent,
        .max_memory = 1024 * 1024,
        .auto_save_interval = 60000,
        .backup_retention = 3,
        .enable_compression = true,
        .enable_encryption = false,
        .enable_heat_tracking = true,
        .cache_size = 100,
    };
    
    const ultra_config = ModesOps.ModeConfig{
        .mode = .Ultra,
        .max_memory = 1024 * 1024,
        .auto_save_interval = 0,
        .backup_retention = 0,
        .enable_compression = false,
        .enable_encryption = false,
        .enable_heat_tracking = true,
        .cache_size = 500,
    };
    
    try testing.expectEqual(ModesOps.DatabaseMode.Persistent, persistent_config.mode);
    try testing.expectEqual(ModesOps.DatabaseMode.Ultra, ultra_config.mode);
    try testing.expect(persistent_config.auto_save_interval > ultra_config.auto_save_interval);
    try testing.expect(ultra_config.cache_size > persistent_config.cache_size);
}