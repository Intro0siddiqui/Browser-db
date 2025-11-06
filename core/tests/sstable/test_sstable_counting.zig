const std = @import("std");
const Allocator = std.testing.allocator;

test "SstableCounting initialization" {
    const counting = SstableCounting.init(Allocator);
    defer counting.sstable_sizes.deinit();
    
    try std.testing.expectEqual(@as(u64, 0), counting.getTotalCount());
    try std.testing.expectEqual(@as(u64, 0), counting.getTotalSize());
    try std.testing.expectEqual(0.0, counting.getAverageSize());
}

test "SstableCounting adding SSTables" {
    var counting = SstableCounting.init(Allocator);
    defer counting.sstable_sizes.deinit();
    
    // Add some test SSTables
    try counting.countSstable(1, 1024);
    try counting.countSstable(2, 2048);
    try counting.countSstable(3, 3072);
    
    try std.testing.expectEqual(@as(u64, 3), counting.getTotalCount());
    try std.testing.expectEqual(@as(u64, 6144), counting.getTotalSize());
    try std.testing.expectEqual(2048.0, counting.getAverageSize());
}

test "SstableCounting individual SSTable size lookup" {
    var counting = SstableCounting.init(Allocator);
    defer counting.sstable_sizes.deinit();
    
    try counting.countSstable(1, 1024);
    try counting.countSstable(5, 5120);
    
    try std.testing.expectEqual(@as(?u64, 1024), counting.getSizeById(1));
    try std.testing.expectEqual(@as(?u64, 5120), counting.getSizeById(5));
    try std.testing.expectEqual(@as(?u64, null), counting.getSizeById(999));
}

test "SstableCounting statistical functions" {
    var counting = SstableCounting.init(Allocator);
    defer counting.sstable_sizes.deinit();
    
    // Test with uniform sizes
    for (1..5) |i| {
        try counting.countSstable(@intCast(u32, i), 1024);
    }
    
    try std.testing.expectEqual(@as(u64, 4), counting.getTotalCount());
    try std.testing.expectEqual(@as(u64, 4096), counting.getTotalSize());
    try std.testing.expectEqual(1024.0, counting.getAverageSize());
}

test "SstableCounting countSstables function" {
    const counting = try SstableCounting.countSstables("/test/path", Allocator);
    defer counting.sstable_sizes.deinit();
    
    // Based on the test data in the implementation
    try std.testing.expectEqual(@as(u64, 4), counting.getTotalCount());
    try std.testing.expectEqual(@as(u64, 11 * 1024 * 1024), counting.getTotalSize()); // 2MB + 3MB + 1MB + 5MB
}

test "SstableCounting empty state" {
    var counting = SstableCounting.init(Allocator);
    defer counting.sstable_sizes.deinit();
    
    // Should handle empty state gracefully
    try std.testing.expectEqual(@as(u64, 0), counting.getTotalCount());
    try std.testing.expectEqual(@as(u64, 0), counting.getTotalSize());
    try std.testing.expectEqual(0.0, counting.getAverageSize());
    
    // Size lookup should return null for non-existent SSTables
    try std.testing.expectEqual(@as(?u64, null), counting.getSizeById(1));
}

test "SstableCounting statistics consistency" {
    var counting = SstableCounting.init(Allocator);
    defer counting.sstable_sizes.deinit();
    
    // Add SSTables and verify statistics remain consistent
    const test_sizes = [_]u64{ 1024, 2048, 4096, 8192 };
    
    for (test_sizes) |size, i| {
        try counting.countSstable(@intCast(u32, i), size);
        
        const expected_count = @intCast(u64, i + 1);
        const expected_total = test_sizes[0..i + 1];
        
        try std.testing.expectEqual(expected_count, counting.getTotalCount());
        try std.testing.expectEqual(@as(u64, expected_total), counting.getTotalSize());
        
        if (expected_count > 0) {
            const expected_avg = @as(f64, expected_total) / @as(f64, expected_count);
            try std.testing.expectEqual(expected_avg, counting.getAverageSize());
        }
    }
}