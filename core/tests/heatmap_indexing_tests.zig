//! HeatMap Indexing System Tests
//! 
//! Comprehensive test suite for the HeatMap indexing system including:
//! - Heat tracking algorithm tests
//! - Skip-list hybrid structure tests
//! - Bloom filter functionality tests
//! - Dynamic heat management tests
//! - Integration tests with BrowserDB core

const std = @import("std");
const testing = std.testing;
const bdb_format = @import("bdb_format.zig");
const HeatMap = @import("heatmap_indexing.zig");

test "HeatTracker basic functionality" {
    const allocator = std.testing.allocator;
    
    var tracker = HeatMap.HeatTracker.init(allocator, 1000);
    defer tracker.deinit();
    
    const test_key = bdb_format.BDBKey.fromString("test_key") catch return;
    
    // Initially no heat
    try testing.expectEqual(@as(u32, 0), tracker.getHeat(test_key));
    try testing.expectEqual(false, tracker.isHot(test_key));
    
    // Record read access
    try tracker.recordAccess(test_key, .Read);
    try testing.expect(tracker.getHeat(test_key) > 0);
    
    // Record more accesses to make it hot
    for (0..15) |_| {
        try tracker.recordAccess(test_key, .Read);
    }
    
    try testing.expect(tracker.isHot(test_key));
    
    // Test different query types
    const hot_key = bdb_format.BDBKey.fromString("hot_key") catch return;
    try tracker.recordAccess(hot_key, .Write); // Should increase heat more
    try tracker.recordAccess(hot_key, .Delete); // Should increase heat even more
    try tracker.recordAccess(hot_key, .Compact); // Should increase heat most
    
    const hot_heat = tracker.getHeat(hot_key);
    const test_heat = tracker.getHeat(test_key);
    try testing.expect(hot_heat > test_heat);
}

test "HeatTracker getHotKeys" {
    const allocator = std.testing.allocator;
    
    var tracker = HeatMap.HeatTracker.init(allocator, 1000);
    defer tracker.deinit();
    
    // Create multiple keys with different heat levels
    const hot_key = bdb_format.BDBKey.fromString("hot_key") catch return;
    const medium_key = bdb_format.BDBKey.fromString("medium_key") catch return;
    const cold_key = bdb_format.BDBKey.fromString("cold_key") catch return;
    
    // Make hot_key very hot
    for (0..50) |_| {
        try tracker.recordAccess(hot_key, .Read);
    }
    
    // Make medium_key moderately hot
    for (0..20) |_| {
        try tracker.recordAccess(medium_key, .Read);
    }
    
    // Leave cold_key mostly cold
    for (0..5) |_| {
        try tracker.recordAccess(cold_key, .Read);
    }
    
    // Get top 2 hot keys
    const hot_keys = try tracker.getHotKeys(2);
    defer allocator.free(hot_keys);
    
    try testing.expect(hot_keys.len >= 2);
    
    // Verify hot_key is first (highest heat)
    try testing.expect(std.mem.eql(u8, &hot_keys[0].data, "hot_key"));
    
    // Verify the first two keys have higher heat than cold_key
    const cold_heat = tracker.getHeat(cold_key);
    try testing.expect(tracker.getHeat(hot_keys[0]) > cold_heat);
    if (hot_keys.len > 1) {
        try testing.expect(tracker.getHeat(hot_keys[1]) >= cold_heat);
    }
}

test "BloomFilter basic functionality" {
    const allocator = std.testing.allocator;
    
    var filter = try HeatMap.BloomFilter.init(allocator, 1000, 0.01);
    defer filter.deinit();
    
    const test_key = bdb_format.BDBKey.fromString("test_key") catch return;
    const another_key = bdb_format.BDBKey.fromString("another_key") catch return;
    
    // Initially should not contain key
    try testing.expectEqual(false, filter.mightContain(test_key));
    try testing.expectEqual(false, filter.mightContain(another_key));
    
    // Add first key
    try filter.add(test_key);
    
    // Should now contain first key but not second
    try testing.expectEqual(true, filter.mightContain(test_key));
    try testing.expectEqual(false, filter.mightContain(another_key));
    
    // Add second key
    try filter.add(another_key);
    
    // Should now contain both keys
    try testing.expectEqual(true, filter.mightContain(test_key));
    try testing.expectEqual(true, filter.mightContain(another_key));
    
    // Test false positive rate
    const false_positive_rate = filter.getFalsePositiveRate();
    try testing.expect(false_positive_rate >= 0.0);
    try testing.expect(false_positive_rate <= 0.1); // Should be close to 1%
}

test "BloomFilter hash functions" {
    const allocator = std.testing.allocator;
    
    // Test Murmur3 hash
    var filter_murmur3 = try HeatMap.BloomFilter.init(allocator, 100, 0.01);
    defer filter_murmur3.deinit();
    
    const test_key = bdb_format.BDBKey.fromString("test_key") catch return;
    try filter_murmur3.add(test_key);
    try testing.expect(filter_murmur3.mightContain(test_key));
    
    // Test FNV1a hash
    var filter_fnv1a = try HeatMap.BloomFilter.init(allocator, 100, 0.01);
    defer filter_fnv1a.deinit();
    filter_fnv1a.hash_type = .FNV1a;
    try filter_fnv1a.add(test_key);
    try testing.expect(filter_fnv1a.mightContain(test_key));
    
    // Test DJB2 hash
    var filter_djb2 = try HeatMap.BloomFilter.init(allocator, 100, 0.01);
    defer filter_djb2.deinit();
    filter_djb2.hash_type = .DJB2;
    try filter_djb2.add(test_key);
    try testing.expect(filter_djb2.mightContain(test_key));
}

test "HeatSkipList basic functionality" {
    const allocator = std.testing.allocator;
    
    var skip_list = try HeatMap.HeatSkipList.init(allocator, 16);
    defer skip_list.deinit(allocator);
    
    const key1 = bdb_format.BDBKey.fromString("key1") catch return;
    const key2 = bdb_format.BDBKey.fromString("key2") catch return;
    const key3 = bdb_format.BDBKey.fromString("key3") catch return;
    
    const value1 = bdb_format.BDBValue.fromString("value1") catch return;
    const value2 = bdb_format.BDBValue.fromString("value2") catch return;
    const value3 = bdb_format.BDBValue.fromString("value3") catch return;
    
    // Insert with different heat levels
    try skip_list.insert(allocator, key1, value1, 10);
    try skip_list.insert(allocator, key2, value2, 30); // Higher heat
    try skip_list.insert(allocator, key3, value3, 5);  // Lower heat
    
    // Search for inserted keys
    const node1 = skip_list.search(key1);
    try testing.expect(node1 != null);
    if (node1) |node| {
        try testing.expectEqual(@as(u32, 10), node.heat);
    }
    
    const node2 = skip_list.search(key2);
    try testing.expect(node2 != null);
    if (node2) |node| {
        try testing.expectEqual(@as(u32, 30), node.heat);
    }
    
    const node3 = skip_list.search(key3);
    try testing.expect(node3 != null);
    if (node3) |node| {
        try testing.expectEqual(@as(u32, 5), node.heat);
    }
    
    // Test get hot keys
    const hot_keys = try skip_list.getHotKeys(20);
    defer allocator.free(hot_keys);
    
    try testing.expect(hot_keys.len == 3);
    
    // Verify ordering by heat (highest heat first)
    for (0..hot_keys.len - 1) |i| {
        const current_heat = skip_list.search(hot_keys[i]).?.heat;
        const next_heat = skip_list.search(hot_keys[i + 1]).?.heat;
        try testing.expect(current_heat >= next_heat);
    }
}

test "DynamicHeatManager integration" {
    const allocator = std.testing.allocator;
    
    var manager = try HeatMap.DynamicHeatManager.init(allocator, 1000, 16);
    defer manager.deinit(allocator);
    
    const key1 = bdb_format.BDBKey.fromString("key1") catch return;
    const key2 = bdb_format.BDBKey.fromString("key2") catch return;
    
    const value1 = bdb_format.BDBValue.fromString("value1") catch return;
    const value2 = bdb_format.BDBValue.fromString("value2") catch return;
    
    // Record accesses for key1
    for (0..20) |_| {
        try manager.recordAccess(key1, value1, .Read);
    }
    
    // Record accesses for key2 (fewer)
    for (0..5) |_| {
        try manager.recordAccess(key2, value2, .Read);
    }
    
    // Test heat detection
    try testing.expect(manager.isHot(key1));
    try testing.expect(!manager.isHot(key2));
    
    // Test bloom filter
    try testing.expect(manager.mightContain(key1));
    try testing.expect(manager.mightContain(key2));
    
    const cold_key = bdb_format.BDBKey.fromString("cold_key") catch return;
    try testing.expect(!manager.mightContain(cold_key));
    
    // Test heat levels
    const heat1 = manager.getHeat(key1);
    const heat2 = manager.getHeat(key2);
    try testing.expect(heat1 > heat2);
}

test "DynamicHeatManager threshold adaptation" {
    const allocator = std.testing.allocator;
    
    var manager = try HeatMap.DynamicHeatManager.init(allocator, 1000, 16);
    defer manager.deinit(allocator);
    
    const initial_threshold = manager.adapt_thresholds.hot_threshold;
    
    // Create many hot keys to trigger threshold adaptation
    for (0..30) |i| {
        const key = bdb_format.BDBKey.fromString(std.fmt.allocPrint(allocator, "key_{d}", .{i}) catch return) catch return;
        const value = bdb_format.BDBValue.fromString(std.fmt.allocPrint(allocator, "value_{d}", .{i}) catch return) catch return;
        
        // Make each key moderately hot
        for (0..30) |_| {
            try manager.recordAccess(key, value, .Read);
        }
    }
    
    // Adapt thresholds
    manager.adaptThresholds();
    
    // Threshold should have changed based on access patterns
    try testing.expect(manager.adapt_thresholds.hot_threshold != initial_threshold);
}

test "HeatAwareBrowserDB workflow" {
    const allocator = std.testing.allocator;
    
    // This test would require integration with the full BrowserDB
    // For now, we'll test the heat manager independently
    var manager = try HeatMap.DynamicHeatManager.init(allocator, 1000, 16);
    defer manager.deinit(allocator);
    
    const test_key = bdb_format.BDBKey.fromString("workflow_test_key") catch return;
    const test_value = bdb_format.BDBValue.fromString("workflow_test_value") catch return;
    
    // Simulate a workflow: reads, writes, and queries
    for (0..10) |i| {
        try manager.recordAccess(test_key, test_value, .Read);
        
        // Every few iterations, do a write
        if (i % 3 == 0) {
            try manager.recordAccess(test_key, test_value, .Write);
        }
    }
    
    // Should be hot by now
    try testing.expect(manager.isHot(test_key));
    
    const heat_level = manager.getHeat(test_key);
    try testing.expect(heat_level > 15); // 10 reads + ~3 writes
    
    // Get hot keys
    const hot_keys = try manager.getHotKeys(10);
    defer allocator.free(hot_keys);
    
    try testing.expect(hot_keys.len >= 1);
    try testing.expect(std.mem.eql(u8, &hot_keys[0].data, &test_key.data));
}

test "HeatMap performance characteristics" {
    const allocator = std.testing.allocator;
    
    const start_time = std.time.milliTimestamp();
    
    var tracker = HeatMap.HeatTracker.init(allocator, 10000);
    defer tracker.deinit();
    
    var manager = try HeatMap.DynamicHeatManager.init(allocator, 10000, 16);
    defer manager.deinit(allocator);
    
    var filter = try HeatMap.BloomFilter.init(allocator, 10000, 0.01);
    defer filter.deinit();
    
    // Simulate high-frequency access pattern
    const num_keys = 100;
    const accesses_per_key = 50;
    
    for (0..num_keys) |i| {
        const key_data = std.fmt.allocPrint(allocator, "perf_key_{d}", .{i}) catch return;
        const key = bdb_format.BDBKey.fromString(key_data) catch return;
        defer allocator.free(key_data);
        
        const value_data = std.fmt.allocPrint(allocator, "perf_value_{d}", .{i}) catch return;
        const value = bdb_format.BDBValue.fromString(value_data) catch return;
        defer allocator.free(value_data);
        
        for (0..accesses_per_key) |_| {
            try manager.recordAccess(key, value, .Read);
            try filter.add(key);
        }
    }
    
    const end_time = std.time.milliTimestamp();
    const total_time = end_time - start_time;
    
    // Should complete within reasonable time (less than 10 seconds)
    try testing.expect(total_time < 10000);
    
    // Test bloom filter performance
    const bloom_start = std.time.milliTimestamp();
    for (0..num_keys) |i| {
        const key_data = std.fmt.allocPrint(allocator, "perf_key_{d}", .{i}) catch return;
        const key = bdb_format.BDBKey.fromString(key_data) catch return;
        defer allocator.free(key_data);
        
        _ = filter.mightContain(key);
    }
    const bloom_end = std.time.milliTimestamp();
    const bloom_time = bloom_end - bloom_start;
    
    // Bloom filter lookups should be very fast (less than 1 second)
    try testing.expect(bloom_time < 1000);
    
    std.debug.print("HeatMap Performance: {d}ms total, {d}ms for {d} bloom filter lookups\n", .{
        total_time, bloom_time, num_keys
    });
}

test "HeatMap memory efficiency" {
    const allocator = std.testing.allocator;
    
    var tracker = HeatMap.HeatTracker.init(allocator, 1000);
    defer tracker.deinit();
    
    // Track many keys
    const num_keys = 500;
    for (0..num_keys) |i| {
        const key_data = std.fmt.allocPrint(allocator, "memory_key_{d}", .{i}) catch return;
        const key = bdb_format.BDBKey.fromString(key_data) catch return;
        defer allocator.free(key_data);
        
        // Add some with low access, some with high access
        const accesses = if (i % 4 == 0) 20 else 2;
        for (0..accesses) |_| {
            try tracker.recordAccess(key, .Read);
        }
    }
    
    // Apply decay to remove low-heat entries
    tracker.applyDecay();
    
    // Should have removed many low-heat entries
    try testing.expect(tracker.current_entries < num_keys);
    
    std.debug.print("Memory Efficiency: {d} entries after decay from {d} initial\n", .{
        tracker.current_entries, num_keys
    });
}

test "HeatMap stress test" {
    const allocator = std.testing.allocator;
    
    var manager = try HeatMap.DynamicHeatManager.init(allocator, 5000, 16);
    defer manager.deinit(allocator);
    
    // Stress test with rapid access patterns
    const stress_cycles = 1000;
    const keys_per_cycle = 10;
    
    for (0..stress_cycles) |cycle| {
        for (0..keys_per_cycle) |i| {
            const key_data = std.fmt.allocPrint(allocator, "stress_key_{d}_{d}", .{cycle, i}) catch return;
            const key = bdb_format.BDBKey.fromString(key_data) catch return;
            defer allocator.free(key_data);
            
            const value_data = std.fmt.allocPrint(allocator, "stress_value_{d}_{d}", .{cycle, i}) catch return;
            const value = bdb_format.BDBValue.fromString(value_data) catch return;
            defer allocator.free(value_data);
            
            // Mix of different query types
            const query_type = switch (i % 4) {
                0 => HeatMap.QueryType.Read,
                1 => HeatMap.QueryType.Write,
                2 => HeatMap.QueryType.Delete,
                else => HeatMap.QueryType.Compact,
            };
            
            try manager.recordAccess(key, value, query_type);
        }
        
        // Periodically adapt thresholds
        if (cycle % 100 == 0) {
            manager.adaptThresholds();
        }
    }
    
    // Should handle stress without crashes
    try testing.expect(true);
    
    std.debug.print("Stress Test: Completed {d} cycles with {d} keys per cycle\n", .{
        stress_cycles, keys_per_cycle
    });
}

// Integration test helper function
fn createTestKey(allocator: mem.Allocator, prefix: []const u8, suffix: anytype) !bdb_format.BDBKey {
    const key_data = try std.fmt.allocPrint(allocator, "{s}_{any}", .{ prefix, suffix });
    defer allocator.free(key_data);
    return bdb_format.BDBKey.fromString(key_data);
}

fn createTestValue(allocator: mem.Allocator, prefix: []const u8, suffix: anytype) !bdb_format.BDBValue {
    const value_data = try std.fmt.allocPrint(allocator, "{s}_{any}", .{ prefix, suffix });
    defer allocator.free(value_data);
    return bdb_format.BDBValue.fromString(value_data);
}