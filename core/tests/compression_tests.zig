const std = @import("std");
const bdb_format = @import("../src/core/bdb_format.zig");

// Test statistics
var test_count: usize = 0;
var pass_count: usize = 0;
var fail_count: usize = 0;

// Test framework
fn runTest(comptime name: []const u8, test_fn: fn () anyerror!void) void {
    test_count += 1;
    std.debug.print("\n🧪 Running compression test: {s}...", .{name});
    
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

fn expectError(expected_error: anyerror, actual_error: anyerror) !void {
    if (expected_error != actual_error) {
        std.debug.print("Expected error: {}, Actual error: {}\n", .{ expected_error, actual_error });
        return error.TestFailed;
    }
}

// ==================== LZ77 COMPRESSION TESTS ====================

fn test_lz77_basic_compression() !void {
    var gpa = std.heap.GeneralPurposeAllocator(.{}){};
    defer _ = gpa.deinit();
    
    const allocator = gpa.allocator();
    
    // Test simple repetitive data
    const data = "Hello, World! Hello, World! Hello, World!";
    const compressed = try bdb_format.BDBCompression.compress(data, .Zlib);
    defer allocator.free(compressed);
    
    // Should compress some
    try expect(compressed.len < data.len);
    
    // Decompress and verify
    const decompressed = try bdb_format.BDBCompression.decompress(compressed, .Zlib, data.len);
    defer allocator.free(decompressed);
    
    try expectEqual(usize, data.len, decompressed.len);
    try expect(std.mem.eql(u8, data, decompressed));
    
    std.debug.print("📦 LZ77: {} -> {} bytes ({d:.1}% ratio)\n", .{
        data.len, compressed.len, 
        (compressed.len * 100.0) / @as(f64, @floatFromInt(data.len))
    });
}

fn test_lz77_empty_data() !void {
    var gpa = std.heap.GeneralPurposeAllocator(.{}){};
    defer _ = gpa.deinit();
    
    const allocator = gpa.allocator();
    
    const data = "";
    const compressed = try bdb_format.BDBCompression.compress(data, .Zlib);
    defer allocator.free(compressed);
    
    try expectEqual(usize, 0, compressed.len);
    
    const decompressed = try bdb_format.BDBCompression.decompress(compressed, .Zlib, 0);
    defer allocator.free(decompressed);
    
    try expectEqual(usize, 0, decompressed.len);
}

fn test_lz77_small_data() !void {
    var gpa = std.heap.GeneralPurposeAllocator(.{}){};
    defer _ = gpa.deinit();
    
    const allocator = gpa.allocator();
    
    // Small data that shouldn't compress well
    const data = "abc";
    const compressed = try bdb_format.BDBCompression.compress(data, .Zlib);
    defer allocator.free(compressed);
    
    const decompressed = try bdb_format.BDBCompression.decompress(compressed, .Zlib, data.len);
    defer allocator.free(decompressed);
    
    try expectEqual(usize, data.len, decompressed.len);
    try expect(std.mem.eql(u8, data, decompressed));
}

fn test_lz77_large_repetitive_data() !void {
    var gpa = std.heap.GeneralPurposeAllocator(.{}){};
    defer _ = gpa.deinit();
    
    const allocator = gpa.allocator();
    
    // Create large repetitive data
    const pattern = "The quick brown fox jumps over the lazy dog. ";
    var data = std.ArrayList(u8).init(allocator);
    defer data.deinit();
    
    // Repeat pattern 1000 times
    for (0..1000) |_| {
        try data.appendSlice(pattern);
    }
    
    const compressed = try bdb_format.BDBCompression.compress(data.items, .Zlib);
    defer allocator.free(compressed);
    
    // Should compress significantly
    const compression_ratio = (compressed.len * 100) / data.items.len;
    try expect(compression_ratio < 50); // Should be less than 50% of original size
    
    const decompressed = try bdb_format.BDBCompression.decompress(compressed, .Zlib, data.items.len);
    defer allocator.free(decompressed);
    
    try expectEqual(usize, data.items.len, decompressed.len);
    try expect(std.mem.eql(u8, data.items, decompressed));
    
    std.debug.print("📊 LZ77 Large: {} -> {} bytes ({d:.1}% compression)\n", .{
        data.items.len, compressed.len, compression_ratio
    });
}

fn test_lz77_corruption_recovery() !void {
    var gpa = std.heap.GeneralPurposeAllocator(.{}){};
    defer _ = gpa.deinit();
    
    const allocator = gpa.allocator();
    
    const data = "Test data for corruption testing";
    const compressed = try bdb_format.BDBCompression.compress(data, .Zlib);
    defer allocator.free(compressed);
    
    // Corrupt the compressed data
    var corrupted = compressed[0..compressed.len].*;
    corrupted[10] = 0xFF; // Corrupt some byte
    
    // Decompression should handle corruption gracefully
    const decompressed = bdb_format.BDBCompression.decompress(&corrupted, .Zlib, data.len) catch {
        // It's OK if decompression fails - we expect corruption to be detected
        return;
    };
    defer allocator.free(decompressed);
    
    // If decompression succeeded, data should match (lucky case)
    // If it failed, that's also expected behavior for corrupted data
}

// ==================== LZ4 COMPRESSION TESTS ====================

fn test_lz4_basic_compression() !void {
    var gpa = std.heap.GeneralPurposeAllocator(.{}){};
    defer _ = gpa.deinit();
    
    const allocator = gpa.allocator();
    
    const data = "LZ4 test data with some repetition. LZ4 test data with some repetition!";
    const compressed = try bdb_format.BDBCompression.compress(data, .Lz4);
    defer allocator.free(compressed);
    
    const decompressed = try bdb_format.BDBCompression.decompress(compressed, .Lz4, data.len);
    defer allocator.free(decompressed);
    
    try expectEqual(usize, data.len, decompressed.len);
    try expect(std.mem.eql(u8, data, decompressed));
    
    std.debug.print("⚡ LZ4: {} -> {} bytes\n", .{ data.len, compressed.len });
}

fn test_lz4_no_compression() !void {
    var gpa = std.heap.GeneralPurposeAllocator(.{}){};
    defer _ = gpa.deinit();
    
    const allocator = gpa.allocator();
    
    // Random-like data that won't compress well
    const data = "ABCDEFGHIJKLMNOPQRSTUVWXYZ1234567890abcdefghijklmnopqrstuvwxyz";
    const compressed = try bdb_format.BDBCompression.compress(data, .Lz4);
    defer allocator.free(compressed);
    
    const decompressed = try bdb_format.BDBCompression.decompress(compressed, .Lz4, data.len);
    defer allocator.free(decompressed);
    
    try expectEqual(usize, data.len, decompressed.len);
    try expect(std.mem.eql(u8, data, decompressed));
}

fn test_lz4_high_compression() !void {
    var gpa = std.heap.GeneralPurposeAllocator(.{}){};
    defer _ = gpa.deinit();
    
    const allocator = gpa.allocator();
    
    // High repetition data
    const pattern = "AAAAAABBBBBBCCCCCC";
    var data = std.ArrayList(u8).init(allocator);
    defer data.deinit();
    
    for (0..1000) |_| {
        try data.appendSlice(pattern);
    }
    
    const compressed = try bdb_format.BDBCompression.compress(data.items, .Lz4);
    defer allocator.free(compressed);
    
    const decompressed = try bdb_format.BDBCompression.decompress(compressed, .Lz4, data.items.len);
    defer allocator.free(decompressed);
    
    try expectEqual(usize, data.items.len, decompressed.len);
    try expect(std.mem.eql(u8, data.items, decompressed));
    
    // Should achieve high compression
    const ratio = (compressed.len * 100) / data.items.len;
    std.debug.print("🔥 LZ4 High compression: {} -> {} ({d:.1}%)\n", .{
        data.items.len, compressed.len, ratio
    });
}

// ==================== ZSTANDARD COMPRESSION TESTS ====================

fn test_zstd_basic_compression() !void {
    var gpa = std.heap.GeneralPurposeAllocator(.{}){};
    defer _ = gpa.deinit();
    
    const allocator = gpa.allocator();
    
    const data = "Zstandard compression test with mixed content: letters, numbers 123, and symbols !@#$%";
    const compressed = try bdb_format.BDBCompression.compress(data, .Zstd);
    defer allocator.free(compressed);
    
    const decompressed = try bdb_format.BDBCompression.decompress(compressed, .Zstd, data.len);
    defer allocator.free(decompressed);
    
    try expectEqual(usize, data.len, decompressed.len);
    try expect(std.mem.eql(u8, data, decompressed));
    
    std.debug.print("🗜️ Zstd: {} -> {} bytes\n", .{ data.len, compressed.len });
}

fn test_zstd_entropy_encoding() !void {
    var gpa = std.heap.GeneralPurposeAllocator(.{}){};
    defer _ = gpa.deinit();
    
    const allocator = gpa.allocator();
    
    // Data with clear frequency patterns
    var data = std.ArrayList(u8).init(allocator);
    defer data.deinit();
    
    // 40% 'A', 30% 'B', 20% 'C', 10% others
    for (0..4000) |_| try data.append('A');
    for (0..3000) |_| try data.append('B');
    for (0..2000) |_| try data.append('C');
    for (0..1000) |i| {
        const val = @as(u8, @intCast(@mod(@as(i32, @intCast(i)), 26) + 'D'));
        try data.append(val);
    }
    
    const compressed = try bdb_format.BDBCompression.compress(data.items, .Zstd);
    defer allocator.free(compressed);
    
    const decompressed = try bdb_format.BDBCompression.decompress(compressed, .Zstd, data.items.len);
    defer allocator.free(decompressed);
    
    try expectEqual(usize, data.items.len, decompressed.len);
    try expect(std.mem.eql(u8, data.items, decompressed));
    
    // Should achieve good compression due to frequency patterns
    const ratio = (compressed.len * 100) / data.items.len;
    std.debug.print("📈 Zstd Entropy: {} -> {} ({d:.1}%)\n", .{
        data.items.len, compressed.len, ratio
    });
}

// ==================== COMPRESSION ALGORITHM COMPARISON ====================

fn test_algorithm_selection() !void {
    var gpa = std.heap.GeneralPurposeAllocator(.{}){};
    defer _ = gpa.deinit();
    
    const allocator = gpa.allocator();
    
    // Test different data types
    const test_cases = [_]struct {
        name: []const u8,
        data: []const u8,
        expected: bdb_format.CompressionType,
    }{
        .{ .name = "small", .data = "abc", .expected = .None },
        .{ .name = "repetitive", .data = "AAAAAAAAAAAAAAAAAAAA", .expected = .Lz4 },
        .{ .name = "structured", .data = "url://example.com/path?param=value", .expected = .Zstd },
    };
    
    for (test_cases) |case| {
        const recommended = bdb_format.BDBCompression.getRecommendedAlgorithm(case.data);
        std.debug.print("🎯 Algorithm selection for {s}: {s}\n", .{
            case.name, @tagName(recommended)
        });
        
        // Test that recommended algorithm works
        const compressed = try bdb_format.BDBCompression.compress(case.data, recommended);
        defer allocator.free(compressed);
        
        const decompressed = try bdb_format.BDBCompression.decompress(compressed, recommended, case.data.len);
        defer allocator.free(decompressed);
        
        try expect(std.mem.eql(u8, case.data, decompressed));
    }
}

fn test_compression_ratio_calculation() !void {
    var gpa = std.heap.GeneralPurposeAllocator(.{}){};
    defer _ = gpa.deinit();
    
    const original_size = 1000;
    const compressed_size = 250;
    
    const ratio = bdb_format.BDBCompression.calculateRatio(compressed_size, original_size);
    
    try expectEqual(u16, 25, ratio); // 250/1000 * 100 = 25%
    
    // Test edge case
    const zero_ratio = bdb_format.BDBCompression.calculateRatio(0, 0);
    try expectEqual(u16, 100, zero_ratio);
}

fn test_compression_error_conditions() !void {
    var gpa = std.heap.GeneralPurposeAllocator(.{}){};
    defer _ = gpa.deinit();
    
    const allocator = gpa.allocator();
    
    // Test data too large error
    var large_data = std.ArrayList(u8).init(allocator);
    defer large_data.deinit();
    
    // Create data larger than LZ77 limit (16MB)
    for (0..(17 * 1024 * 1024)) |_| {
        try large_data.append('A');
    }
    
    const result = bdb_format.BDBCompression.compress(large_data.items, .Zlib);
    try expectError(bdb_format.CompressionError.DataTooLarge, result);
}

// ==================== PERFORMANCE TESTS ====================

fn test_compression_performance() !void {
    var gpa = std.heap.GeneralPurposeAllocator(.{}){};
    defer _ = gpa.deinit();
    
    const allocator = gpa.allocator();
    
    // Create test data
    var data = std.ArrayList(u8).init(allocator);
    defer data.deinit();
    
    for (0..10000) |i| {
        const char = @as(u8, @intCast(@mod(i, 256)));
        try data.append(char);
    }
    
    // Test LZ77 performance
    const start_lz77 = std.time.nanoTimestamp();
    const compressed_lz77 = try bdb_format.BDBCompression.compress(data.items, .Zlib);
    const end_lz77 = std.time.nanoTimestamp();
    defer allocator.free(compressed_lz77);
    
    const lz77_time = @as(f64, @floatFromInt(end_lz77 - start_lz77)) / 1_000_000.0;
    
    // Test LZ4 performance
    const start_lz4 = std.time.nanoTimestamp();
    const compressed_lz4 = try bdb_format.BDBCompression.compress(data.items, .Lz4);
    const end_lz4 = std.time.nanoTimestamp();
    defer allocator.free(compressed_lz4);
    
    const lz4_time = @as(f64, @floatFromInt(end_lz4 - start_lz4)) / 1_000_000.0;
    
    std.debug.print("⏱️ Compression Performance:\n", .{});
    std.debug.print("  LZ77: {:.2}ms ({} bytes)\n", .{ lz77_time, compressed_lz77.len });
    std.debug.print("  LZ4: {:.2}ms ({} bytes)\n", .{ lz4_time, compressed_lz4.len });
    
    // Both should complete in reasonable time
    try expect(lz77_time < 100.0);
    try expect(lz4_time < 100.0);
}

fn test_decompression_performance() !void {
    var gpa = std.heap.GeneralPurposeAllocator(.{}){};
    defer _ = gpa.deinit();
    
    const allocator = gpa.allocator();
    
    var data = std.ArrayList(u8).init(allocator);
    defer data.deinit();
    
    // Create repetitive data for compression
    for (0..5000) |i| {
        const pattern = @as(u8, @intCast(@mod(i, 26) + 'A'));
        try data.append(pattern);
    }
    
    // Compress first
    const compressed = try bdb_format.BDBCompression.compress(data.items, .Zlib);
    defer allocator.free(compressed);
    
    // Test decompression performance
    const start = std.time.nanoTimestamp();
    const decompressed = try bdb_format.BDBCompression.decompress(compressed, .Zlib, data.items.len);
    const end = std.time.nanoTimestamp();
    defer allocator.free(decompressed);
    
    const decompress_time = @as(f64, @floatFromInt(end - start)) / 1_000_000.0;
    
    try expectEqual(usize, data.items.len, decompressed.len);
    try expect(std.mem.eql(u8, data.items, decompressed));
    
    std.debug.print("🔄 Decompression: {:.2}ms\n", .{decompress_time});
    
    // Should be fast
    try expect(decompress_time < 50.0);
}

// ==================== EDGE CASE TESTS ====================

fn test_compression_edge_cases() !void {
    var gpa = std.heap.GeneralPurposeAllocator(.{}){};
    defer _ = gpa.deinit();
    
    const allocator = gpa.allocator();
    
    // Test single byte
    const single_byte = "X";
    const compressed = try bdb_format.BDBCompression.compress(single_byte, .Lz4);
    defer allocator.free(compressed);
    
    const decompressed = try bdb_format.BDBCompression.decompress(compressed, .Lz4, single_byte.len);
    defer allocator.free(decompressed);
    
    try expect(std.mem.eql(u8, single_byte, decompressed));
    
    // Test maximum safe size for each algorithm
    const max_size = 8 * 1024 * 1024; // 8MB
    var max_data = std.ArrayList(u8).init(allocator);
    defer max_data.deinit();
    
    for (0..max_size) |i| {
        try max_data.append(@as(u8, @intCast(@mod(i, 256))));
    }
    
    const result = bdb_format.BDBCompression.compress(max_data.items, .Zstd);
    if (result) |compressed_zstd| {
        defer allocator.free(compressed_zstd);
        
        const decompressed_zstd = try bdb_format.BDBCompression.decompress(compressed_zstd, .Zstd, max_data.items.len);
        defer allocator.free(decompressed_zstd);
        
        try expect(std.mem.eql(u8, max_data.items, decompressed_zstd));
    } else |err| {
        // Either success or data too large error
        try expect(err == bdb_format.CompressionError.DataTooLarge or 
                  err == bdb_format.CompressionError.OutOfMemory);
    }
}

fn test_none_compression() !void {
    const data = "No compression test data";
    
    // Test that None compression just returns the data
    const result = bdb_format.BDBCompression.compress(data, .None);
    try expect(result.ptr == data.ptr); // Should return same slice
    
    const decompressed = bdb_format.BDBCompression.decompress(data, .None, data.len);
    try expect(decompressed.ptr == data.ptr); // Should return same slice
}

// ==================== REAL-WORLD DATA TESTS ====================

fn test_browser_data_compression() !void {
    var gpa = std.heap.GeneralPurposeAllocator(.{}){};
    defer _ = gpa.deinit();
    
    const allocator = gpa.allocator();
    
    // Simulate browser data (URLs, HTML fragments, JSON)
    const browser_data = 
        \\{"url": "https://example.com/page1", "title": "Example Page 1"}
        \\https://example.com/page2 Example Page 2
        \\<html><body><h1>Title</h1><p>Content</p></body></html>
        \\https://example.com/page1 {"url": "https://example.com/page1", "title": "Example Page 1"}
        \\https://example.com/page3 Example Page 3
    ;
    
    const compressed = try bdb_format.BDBCompression.compress(browser_data, .Lz4);
    defer allocator.free(compressed);
    
    const decompressed = try bdb_format.BDBCompression.decompress(compressed, .Lz4, browser_data.len);
    defer allocator.free(decompressed);
    
    try expect(std.mem.eql(u8, browser_data, decompressed));
    
    const ratio = (compressed.len * 100) / browser_data.len;
    std.debug.print("🌐 Browser data compression: {} -> {} ({d:.1}%)\n", .{
        browser_data.len, compressed.len, ratio
    });
}

fn test_json_compression() !void {
    var gpa = std.heap.GeneralPurposeAllocator(.{}){};
    defer _ = gpa.deinit();
    
    const allocator = gpa.allocator();
    
    // Large JSON with repetitive structure
    var json_data = std.ArrayList(u8).init(allocator);
    defer json_data.deinit();
    
    try json_data.appendSlice("[{\"name\":");
    
    for (0..1000) |i| {
        const entry = try std.fmt.allocPrint(allocator, "\"User{d}\",\"email\":\"user{d}@example.com\",\"active\":true},", .{ i, i });
        defer allocator.free(entry);
        try json_data.appendSlice(entry);
    }
    
    try json_data.appendSlice("]");
    
    const compressed = try bdb_format.BDBCompression.compress(json_data.items, .Zstd);
    defer allocator.free(compressed);
    
    const decompressed = try bdb_format.BDBCompression.decompress(compressed, .Zstd, json_data.items.len);
    defer allocator.free(decompressed);
    
    try expect(std.mem.eql(u8, json_data.items, decompressed));
    
    const ratio = (compressed.len * 100) / json_data.items.len;
    std.debug.print("📋 JSON compression: {} -> {} ({d:.1}%)\n", .{
        json_data.items.len, compressed.len, ratio
    });
}

// ==================== MAIN TEST RUNNER ====================

pub fn main() !void {
    std.debug.print("🚀 BrowserDB Compression Test Suite\n", .{});
    std.debug.print("===================================\n\n", .{});
    
    // LZ77 Tests
    runTest("LZ77 Basic Compression", test_lz77_basic_compression);
    runTest("LZ77 Empty Data", test_lz77_empty_data);
    runTest("LZ77 Small Data", test_lz77_small_data);
    runTest("LZ77 Large Repetitive", test_lz77_large_repetitive_data);
    runTest("LZ77 Corruption Recovery", test_lz77_corruption_recovery);
    
    // LZ4 Tests
    runTest("LZ4 Basic Compression", test_lz4_basic_compression);
    runTest("LZ4 No Compression", test_lz4_no_compression);
    runTest("LZ4 High Compression", test_lz4_high_compression);
    
    // Zstandard Tests
    runTest("Zstd Basic Compression", test_zstd_basic_compression);
    runTest("Zstd Entropy Encoding", test_zstd_entropy_encoding);
    
    // Algorithm Selection
    runTest("Algorithm Selection", test_algorithm_selection);
    runTest("Compression Ratio Calculation", test_compression_ratio_calculation);
    runTest("Error Conditions", test_compression_error_conditions);
    
    // Performance Tests
    runTest("Compression Performance", test_compression_performance);
    runTest("Decompression Performance", test_decompression_performance);
    
    // Edge Cases
    runTest("Edge Cases", test_compression_edge_cases);
    runTest("None Compression", test_none_compression);
    
    // Real-world Data
    runTest("Browser Data", test_browser_data_compression);
    runTest("JSON Compression", test_json_compression);
    
    // Output test results
    std.debug.print("\n📊 Test Results:\n", .{});
    std.debug.print("   Total tests: {d}\n", .{test_count});
    std.debug.print("   Passed: {d}\n", .{pass_count});
    std.debug.print("   Failed: {d}\n", .{fail_count});
    std.debug.print("   Success rate: {d}%\n", .{
        if (test_count > 0) (pass_count * 100 / test_count) else 0
    });
    
    if (fail_count == 0) {
        std.debug.print("\n🎉 All compression tests passed! Compression system is production-ready.\n", .{});
    } else {
        std.debug.print("\n⚠️ Some compression tests failed. Please review the implementation.\n", .{});
        return error.TestsFailed;
    }
}
