const std = @import("std");
const bdb_format = @import("bdb_format");

test "BDBCompression - Basic LZ77 compression and decompression" {
    const test_data = "Hello, World! This is a test of LZ77 compression. Hello, World! This should compress well.";
    
    const compressed = try bdb_format.BDBCompression.compress(test_data, .Zlib);
    defer std.testing.allocator.free(compressed);
    
    const decompressed = try bdb_format.BDBCompression.decompress(compressed, .Zlib, test_data.len);
    defer std.testing.allocator.free(decompressed);
    
    try std.testing.expectEqualSlices(u8, test_data, decompressed);
    try std.testing.expect(compressed.len < test_data.len); // Should actually compress
}

test "BDBCompression - Basic LZ4 compression and decompression" {
    const test_data = "AAAAAABBBBBBBBBBCCCCCCDDDDDDDDDDEEEEEEEEEEEE";
    
    const compressed = try bdb_format.BDBCompression.compress(test_data, .Lz4);
    defer std.testing.allocator.free(compressed);
    
    const decompressed = try bdb_format.BDBCompression.decompress(compressed, .Lz4, test_data.len);
    defer std.testing.allocator.free(decompressed);
    
    try std.testing.expectEqualSlices(u8, test_data, decompressed);
}

test "BDBCompression - Algorithm recommendation" {
    const large_data = "A" ** 10000; // 10KB of same character
    
    const rec_large = bdb_format.BDBCompression.getRecommendedAlgorithm(large_data);
    
    // Large repeated data should recommend LZ4 (good for patterns)
    try std.testing.expect(rec_large == .Lz4);
}

test "BDBCompression - Empty data handling" {
    const empty_data = "";
    
    const compressed = try bdb_format.BDBCompression.compress(empty_data, .Zlib);
    defer std.testing.allocator.free(compressed);
    
    const decompressed = try bdb_format.BDBCompression.decompress(compressed, .Zlib, empty_data.len);
    defer std.testing.allocator.free(decompressed);
    
    try std.testing.expectEqualSlices(u8, empty_data, decompressed);
}

test "BDBCompression - Compression ratio calculation" {
    const test_data = "AAAAAAAAAABBBBBBBBBBCCCCCCCCCCDDDDDDDDDDEEEEEEEEEEE";
    
    const ratio = bdb_format.BDBCompression.calculateRatio(test_data.len, test_data.len / 2);
    
    try std.testing.expect(ratio > 0.0);
    try std.testing.expect(ratio <= 1.0);
}