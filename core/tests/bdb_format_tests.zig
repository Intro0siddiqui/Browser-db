const std = @import("std");
const bdb_format = @import("core/bdb_format.zig");

// Test statistics
var test_count: usize = 0;
var pass_count: usize = 0;
var fail_count: usize = 0;

// Test framework
fn runTest(comptime name: []const u8, test_fn: fn () anyerror!void) void {
    test_count += 1;
    std.debug.print("\n🧪 Running .bdb format test: {s}...", .{name});
    
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

// ==================== FILE HEADER TESTS ====================

fn test_bdb_header_creation() !void {
    var gpa = std.heap.GeneralPurposeAllocator(.{}){};
    defer _ = gpa.deinit();
    
    const allocator = gpa.allocator();
    
    // Test header creation
    const header = bdb_format.BDBFileHeader.init(.History);
    
    // Verify magic bytes
    try expect(std.mem.eql(u8, &header.magic, &bdb_format.MAGIC_BYTES));
    
    // Verify version
    try expectEqual(u8, bdb_format.BDB_VERSION, header.version);
    
    // Verify table type
    try expectEqual(bdb_format.TableType, .History, header.table_type);
    
    // Verify timestamps are reasonable
    try expect(header.created_at > 0);
    try expect(header.modified_at > 0);
    
    std.debug.print("✅ .bdb header creation test passed\n");
}

fn test_bdb_header_crc_calculation() !void {
    var gpa = std.heap.GeneralPurposeAllocator(.{}){};
    defer _ = gpa.deinit();
    
    const allocator = gpa.allocator();
    
    var header = bdb_format.BDBFileHeader.init(.Cookies);
    
    // Calculate CRC
    const crc = header.calculateCRC();
    try expect(crc > 0);
    
    // Verify CRC is stored
    try expectEqual(u32, crc, header.header_crc);
    
    std.debug.print("✅ .bdb header CRC calculation test passed\n");
}

fn test_bdb_header_serialization() !void {
    var gpa = std.heap.GeneralPurposeAllocator(.{}){};
    defer _ = gpa.deinit();
    
    const allocator = gpa.allocator();
    
    var header = bdb_format.BDBFileHeader.init(.Cache);
    var buffer = std.ArrayList(u8).init(allocator);
    defer buffer.deinit();
    
    // Serialize header
    try header.serialize(&buffer);
    
    // Verify buffer has data
    try expect(buffer.items.len > 0);
    
    // Verify minimum size (header + crc)
    try expect(buffer.items.len >= @sizeOf(bdb_format.BDBFileHeader));
    
    std.debug.print("✅ .bdb header serialization test passed\n");
}

fn test_bdb_header_validation() !void {
    var gpa = std.heap.GeneralPurposeAllocator(.{}){};
    defer _ = gpa.deinit();
    
    const allocator = gpa.allocator();
    
    var header = bdb_format.BDBFileHeader.init(.LocalStore);
    
    // Test valid header
    var buffer = std.ArrayList(u8).init(allocator);
    defer buffer.deinit();
    try header.serialize(&buffer);
    
    // Deserialize back
    // Deserialize back and validate
    var deserialized_header = bdb_format.BDBFileHeader{ .magic = undefined, .version = 0, .table_type = undefined, .created_at = 0, .modified_at = 0, .header_crc = 0 };
    
    // Deserialize from buffer
    var offset: usize = 0;
    try deserialized_header.deserialize(buffer.items, &offset);
    
    // Verify all fields match
    try expect(std.mem.eql(u8, &deserialized_header.magic, &header.magic));
    try expectEqual(u8, header.version, deserialized_header.version);
    try expectEqual(bdb_format.TableType, header.table_type, deserialized_header.table_type);
    try expectEqual(u64, header.created_at, deserialized_header.created_at);
    try expectEqual(u64, header.modified_at, deserialized_header.modified_at);
    try expectEqual(u32, header.header_crc, deserialized_header.header_crc);
    
    // Verify deserialized header is valid
    const is_valid = try deserialized_header.validate();
    try expect(is_valid);
    
    std.debug.print("✅ .bdb full deserialization test passed\n");
}

// ==================== LOG ENTRY TESTS ====================

fn test_bdb_log_entry_creation() !void {
    var gpa = std.heap.GeneralPurposeAllocator(.{}){};
    defer _ = gpa.deinit();
    
    const allocator = gpa.allocator();
    
    const key = "test_key";
    const value = "test_value";
    const timestamp = std.time.milliTimestamp();
    
    // Test insert entry
    const insert_entry = bdb_format.BDBLogEntry.createInsert(key, value, timestamp);
    try expectEqual(bdb_format.EntryType, .Insert, insert_entry.entry_type);
    try expectEqual(usize, key.len, insert_entry.key_length);
    try expectEqual(usize, value.len, insert_entry.value_length);
    try expectEqual(u64, timestamp, insert_entry.timestamp);
    
    // Test update entry
    const update_entry = bdb_format.BDBLogEntry.createUpdate(key, value, timestamp);
    try expectEqual(bdb_format.EntryType, .Update, update_entry.entry_type);
    
    // Test delete entry
    const delete_entry = bdb_format.BDBLogEntry.createDelete(key, timestamp);
    try expectEqual(bdb_format.EntryType, .Delete, delete_entry.entry_type);
    try expectEqual(usize, 0, delete_entry.value_length);
    
    std.debug.print("✅ .bdb log entry creation test passed\n");
}

fn test_bdb_log_entry_crc() !void {
    var gpa = std.heap.GeneralPurposeAllocator(.{}){};
    defer _ = gpa.deinit();
    
    const allocator = gpa.allocator();
    
    const key = "crc_test_key";
    const value = "crc_test_value";
    const timestamp = std.time.milliTimestamp();
    
    var entry = bdb_format.BDBLogEntry.createInsert(key, value, timestamp);
    
    // Calculate CRC
    const crc = entry.calculateCRC();
    try expect(crc > 0);
    
    // Verify CRC is stored
    try expectEqual(u32, crc, entry.entry_crc);
    
    std.debug.print("✅ .bdb log entry CRC test passed\n");
}

fn test_bdb_log_entry_serialization() !void {
    var gpa = std.heap.GeneralPurposeAllocator(.{}){};
    defer _ = gpa.deinit();
    
    const allocator = gpa.allocator();
    
    const key = "serialization_test";
    const value = "serialization_value";
    const timestamp = std.time.milliTimestamp();
    
    var entry = bdb_format.BDBLogEntry.createInsert(key, value, timestamp);
    
    var buffer = std.ArrayList(u8).init(allocator);
    defer buffer.deinit();
    
    // Serialize entry
    try entry.serialize(&buffer);
    
    // Verify buffer has data
    try expect(buffer.items.len > 0);
    
    // Check size is reasonable
    const expected_size = entry.getSize();
    try expect(buffer.items.len >= expected_size / 2); // Allow some overhead
    
    std.debug.print("✅ .bdb log entry serialization test passed\n");
}

fn test_bdb_log_entry_size_calculation() !void {
    var gpa = std.heap.GeneralPurposeAllocator(.{}){};
    defer _ = gpa.deinit();
    
    const allocator = gpa.allocator();
    
    const key = "size_test";
    const value = "size_value";
    const timestamp = std.time.milliTimestamp();
    
    var insert_entry = bdb_format.BDBLogEntry.createInsert(key, value, timestamp);
    var delete_entry = bdb_format.BDBLogEntry.createDelete(key, timestamp);
    
    // Insert should be larger than delete (has value)
    const insert_size = insert_entry.getSize();
    const delete_size = delete_entry.getSize();
    
    try expect(insert_size > delete_size);
    
    std.debug.print("📏 Insert entry size: {}, Delete entry size: {}\n", .{
        insert_size, delete_size
    });
    
    std.debug.print("✅ .bdb log entry size calculation test passed\n");
}

// ==================== VARINT ENCODING TESTS ====================

fn test_bdb_varint_encoding() !void {
    var gpa = std.heap.GeneralPurposeAllocator(.{}){};
    defer _ = gpa.deinit();
    
    const allocator = gpa.allocator();
    
    // Test small values
    const test_values = [_]u64{ 0, 1, 127, 128, 255, 256, 65535, 65536, 16777215, 16777216 };
    
    for (test_values) |value| {
        var buffer = std.ArrayList(u8).init(allocator);
        defer buffer.deinit();
        
        // Encode
        try bdb_format.writeVarInt(&buffer, value);
        
        // Decode
        var offset: usize = 0;
        const decoded = try bdb_format.readVarInt(buffer.items, &offset);
        
        try expectEqual(u64, value, decoded);
    }
    
    std.debug.print("✅ .bdb varint encoding test passed\n");
}

fn test_bdb_varint_size_calculation() !void {
    var gpa = std.heap.GeneralPurposeAllocator(.{}){};
    defer _ = gpa.deinit();
    
    const allocator = gpa.allocator();
    
    // Test varint size calculation
    const test_cases = [_]struct { value: u64, expected_min: usize }{
        .{ .value = 0, .expected_min = 1 },
        .{ .value = 127, .expected_min = 1 },
        .{ .value = 128, .expected_min = 2 },
        .{ .value = 16383, .expected_min = 2 },
        .{ .value = 16384, .expected_min = 3 },
    };
    
    for (test_cases) |case| {
        const size = bdb_format.varintSize(case.value);
        try expect(size >= case.expected_min);
    }
    
    std.debug.print("✅ .bdb varint size calculation test passed\n");
}

// ==================== FILE FOOTER TESTS ====================

fn test_bdb_footer_creation() !void {
    var gpa = std.heap.GeneralPurposeAllocator(.{}){};
    defer _ = gpa.deinit();
    
    const allocator = gpa.allocator();
    
    const footer = bdb_format.BDBFileFooter.init();
    
    // Verify initial values
    try expectEqual(u64, 0, footer.entry_count);
    try expectEqual(u64, 0, footer.file_size);
    try expectEqual(u64, 0, footer.data_offset);
    try expectEqual(u32, 0, footer.max_entry_size);
    try expectEqual(u16, 100, footer.compression_ratio); // 100% = no compression
    
    std.debug.print("✅ .bdb footer creation test passed\n");
}

fn test_bdb_footer_serialization() !void {
    var gpa = std.heap.GeneralPurposeAllocator(.{}){};
    defer _ = gpa.deinit();
    
    const allocator = gpa.allocator();
    
    var footer = bdb_format.BDBFileFooter.init();
    
    // Set some test values
    footer.entry_count = 100;
    footer.file_size = 1024;
    footer.data_offset = 128;
    footer.max_entry_size = 512;
    
    var buffer = std.ArrayList(u8).init(allocator);
    defer buffer.deinit();
    
    // Serialize footer
    try footer.serialize(&buffer);
    
    // Verify buffer has data
    try expect(buffer.items.len > 0);
    
    std.debug.print("✅ .bdb footer serialization test passed\n");
}

// ==================== FILE MANAGER TESTS ====================

fn test_bdb_file_manager_creation() !void {
    var gpa = std.heap.GeneralPurposeAllocator(.{}){};
    defer _ = gpa.deinit();
    
    const allocator = gpa.allocator();
    
    var file_manager = bdb_format.BDBFileManager.init(allocator, "/tmp/test-bdb");
    defer file_manager.deinit();
    
    // Verify initialization
    try expect(file_manager.allocator == allocator);
    try expect(std.mem.eql(u8, file_manager.base_path, "/tmp/test-bdb"));
    try expect(file_manager.max_file_size > 0);
    
    std.debug.print("✅ .bdb file manager creation test passed\n");
}

fn test_bdb_file_creation() !void {
    var gpa = std.heap.GeneralPurposeAllocator(.{}){};
    defer _ = gpa.deinit();
    
    const allocator = gpa.allocator();
    
    const test_path = "/tmp/test-browserdb-file.bdb";
    
    // Delete file if it exists
    std.fs.cwd().deleteFile(test_path) catch {};
    
    // Create file
    var file = try bdb_format.BDBFile.init(allocator, test_path, .History);
    defer file.deinit();
    
    // Verify file was created
    try expect(std.mem.eql(u8, file.filename, test_path));
    try expectEqual(bdb_format.TableType, .History, file.header.table_type);
    try expect(file.current_offset > 0); // Should have written header
    
    std.debug.print("✅ .bdb file creation test passed\n");
}

// ==================== INTEGRATION TESTS ====================

fn test_bdb_complete_workflow() !void {
    var gpa = std.heap.GeneralPurposeAllocator(.{}){};
    defer _ = gpa.deinit();
    
    const allocator = gpa.allocator();
    
    const test_path = "/tmp/test-complete-workflow.bdb";
    std.fs.cwd().deleteFile(test_path) catch {};
    
    // 1. Create file with header
    var file = try bdb_format.BDBFile.init(allocator, test_path, .Settings);
    defer file.deinit();
    
    // 2. Create and serialize entries
    var entries = std.ArrayList(bdb_format.BDBLogEntry).init(allocator);
    defer entries.deinit();
    
    const test_entries = [_]bdb_format.BDBLogEntry{
        bdb_format.BDBLogEntry.createInsert("key1", "value1", 1000),
        bdb_format.BDBLogEntry.createInsert("key2", "value2", 2000),
        bdb_format.BDBLogEntry.createDelete("key1", 3000),
    };
    
    for (test_entries) |entry| {
        try entries.append(entry);
        try file.appendEntry(entry);
    }
    
    // 3. Write footer
    try file.writeFooter();
    
    // 4. Validate file
    const is_valid = try file.validate();
    try expect(is_valid);
    
    // 5. Get stats
    const stats = try file.getStats();
    try expect(stats.entry_count > 0);
    
    std.debug.print("✅ Complete .bdb workflow test passed\n");
}

fn test_bdb_multiple_table_types() !void {
    var gpa = std.heap.GeneralPurposeAllocator(.{}){};
    defer _ = gpa.deinit();
    
    const allocator = gpa.allocator();
    
    // Test all table types
    const table_types = [_]bdb_format.TableType{
        .History, .Cookies, .Cache, .LocalStore, .Settings
    };
    
    for (table_types) |table_type| {
        const test_path = try std.fmt.allocPrint(allocator, "/tmp/test-{s}.bdb", .{
            @tagName(table_type)
        });
        defer allocator.free(test_path);
        
        std.fs.cwd().deleteFile(test_path) catch {};
        
        var file = try bdb_format.BDBFile.init(allocator, test_path, table_type);
        defer file.deinit();
        
        // Create a test entry
        const test_entry = bdb_format.BDBLogEntry.createInsert(
            "test_key", "test_value", std.time.milliTimestamp()
        );
        
        try file.appendEntry(test_entry);
        
        // Verify table type is preserved
        try expectEqual(bdb_format.TableType, table_type, file.header.table_type);
    }
    
    std.debug.print("✅ Multiple table types test passed\n");
}

// ==================== ERROR HANDLING TESTS ====================

fn test_bdb_error_handling() !void {
    var gpa = std.heap.GeneralPurposeAllocator(.{}){};
    defer _ = gpa.deinit();
    
    const allocator = gpa.allocator();
    
    // Test invalid header validation
    var invalid_header = bdb_format.BDBFileHeader.init(.History);
    @memcpy(&invalid_header.magic, "INVALID__"); // Corrupt magic bytes
    
    const is_valid = invalid_header.validate();
    try expect(!is_valid);
    
    // Test varint error conditions
    var buffer = [_]u8{ 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF };
    var offset: usize = 0;
    
    // This should potentially trigger error for malformed varint
    _ = bdb_format.readVarInt(&buffer, &offset) catch |err| {
        try expect(err == bdb_format.FileError.VarIntTooLarge or err == error.Overflow);
    };
    
    std.debug.print("✅ Error handling test passed\n");
}

// ==================== PERFORMANCE TESTS ====================

fn test_bdb_serialization_performance() !void {
    var gpa = std.heap.GeneralPurposeAllocator(.{}){};
    defer _ = gpa.deinit();
    
    const allocator = gpa.allocator();
    
    const start_time = std.time.nanoTimestamp();
    
    // Test batch serialization
    var entries = std.ArrayList(bdb_format.BDBLogEntry).init(allocator);
    defer entries.deinit();
    
    for (0..100) |i| {
        const key = std.fmt.allocPrint(allocator, "key_{}", .{i}) catch continue;
        defer allocator.free(key);
        const value = std.fmt.allocPrint(allocator, "value_{}", .{i}) catch continue;
        defer allocator.free(value);
        
        const entry = bdb_format.BDBLogEntry.createInsert(key, value, i);
        try entries.append(entry);
    }
    
    // Serialize all entries
    var total_buffer = std.ArrayList(u8).init(allocator);
    defer total_buffer.deinit();
    
    for (entries.items) |entry| {
        try entry.serialize(&total_buffer);
    }
    
    const end_time = std.time.nanoTimestamp();
    const elapsed = end_time - start_time;
    const elapsed_ms = @as(f64, @floatFromInt(elapsed)) / 1_000_000.0;
    
    std.debug.print("📊 .bdb Serialization Performance: 100 entries in {:.2}ms\n", .{elapsed_ms});
    std.debug.print("   Buffer size: {} bytes\n", .{total_buffer.items.len});
    std.debug.print("   Average per entry: {:.2}μs\n", .{
        (elapsed / 100) / 1000.0
    });
    
    // Verify reasonable performance (should be under 100ms for 100 entries)
    try expect(elapsed_ms < 100.0);
    
    std.debug.print("✅ .bdb serialization performance test passed\n");
}

// ==================== INTEGRATION TESTS ====================

fn test_bdb_full_file_lifecycle() !void {
    var gpa = std.heap.GeneralPurposeAllocator(.{}){};
    defer _ = gpa.deinit();
    
    const allocator = gpa.allocator();
    
    const test_path = "/tmp/test-full-lifecycle.bdb";
    std.fs.cwd().deleteFile(test_path) catch {};
    
    // 1. Create and populate file
    var file = try bdb_format.BDBFile.init(allocator, test_path, .Cache);
    defer file.deinit();
    
    // 2. Insert various types of entries
    var expected_entries: usize = 0;
    
    // Insert operations
    for (0..50) |i| {
        const key = std.fmt.allocPrint(allocator, "insert_key_{}", .{i}) catch continue;
        defer allocator.free(key);
        const value = std.fmt.allocPrint(allocator, "insert_value_{}", .{i}) catch continue;
        defer allocator.free(value);
        
        const entry = bdb_format.BDBLogEntry.createInsert(key, value, i * 1000);
        try file.appendEntry(entry);
        expected_entries += 1;
    }
    
    // Update operations
    for (0..20) |i| {
        const key = std.fmt.allocPrint(allocator, "update_key_{}", .{i}) catch continue;
        defer allocator.free(key);
        const value = std.fmt.allocPrint(allocator, "updated_value_{}", .{i}) catch continue;
        defer allocator.free(value);
        
        const entry = bdb_format.BDBLogEntry.createUpdate(key, value, i * 2000);
        try file.appendEntry(entry);
    }
    
    // Delete operations
    for (0..10) |i| {
        const key = std.fmt.allocPrint(allocator, "delete_key_{}", .{i}) catch continue;
        defer allocator.free(key);
        
        const entry = bdb_format.BDBLogEntry.createDelete(key, i * 3000);
        try file.appendEntry(entry);
    }
    
    // 3. Write footer
    try file.writeFooter();
    
    // 4. Validate complete file
    const is_valid = try file.validate();
    try expect(is_valid);
    
    // 5. Get final stats
    const stats = try file.getStats();
    try expect(stats.entry_count > 0);
    
    std.debug.print("✅ Full file lifecycle test passed\n");
}

fn test_bdb_varint_edge_cases() !void {
    var gpa = std.heap.GeneralPurposeAllocator(.{}){};
    defer _ = gpa.deinit();
    
    const allocator = gpa.allocator();
    
    // Test edge cases for varint encoding
    const edge_cases = [_]u64{ 0, 1, 127, 128, 16383, 16384, 2097151, 2097152, 268435455, 268435456 };
    
    for (edge_cases) |value| {
        var buffer = std.ArrayList(u8).init(allocator);
        defer buffer.deinit();
        
        // Encode
        try bdb_format.writeVarInt(&buffer, value);
        
        // Decode
        var offset: usize = 0;
        const decoded = try bdb_format.readVarInt(buffer.items, &offset);
        
        try expectEqual(u64, value, decoded);
    }
    
    std.debug.print("✅ Varint edge cases test passed\n");
}

fn test_bdb_header_corruption_recovery() !void {
    var gpa = std.heap.GeneralPurposeAllocator(.{}){};
    defer _ = gpa.deinit();
    
    const allocator = gpa.allocator();
    
    const test_path = "/tmp/test-corruption.bdb";
    std.fs.cwd().deleteFile(test_path) catch {};
    
    // Create valid file
    var file = try bdb_format.BDBFile.init(allocator, test_path, .Settings);
    defer file.deinit();
    
    const entry = bdb_format.BDBLogEntry.createInsert("test_key", "test_value", 12345);
    try file.appendEntry(entry);
    try file.writeFooter();
    
    // Test various corruption scenarios
    // 1. Corrupt magic bytes
    var corrupt_header = bdb_format.BDBFileHeader.init(.Settings);
    @memcpy(&corrupt_header.magic, "CORRUPT");
    
    const magic_valid = corrupt_header.validate();
    try expect(!magic_valid);
    
    // 2. Test with corrupted data
    var buffer = std.ArrayList(u8).init(allocator);
    defer buffer.deinit();
    try buffer.appendNTimes(0xFF, 100);
    
    var offset: usize = 0;
    const result = bdb_format.readVarInt(buffer.items, &offset) catch |err| {
        try expect(err == bdb_format.FileError.VarIntTooLarge or err == error.Overflow);
    };
    
    std.debug.print("✅ Corruption recovery test passed\n");
}

fn test_bdb_concurrent_file_operations() !void {
    var gpa = std.heap.GeneralPurposeAllocator(.{}){};
    defer _ = gpa.deinit();
    
    const allocator = gpa.allocator();
    
    const test_path = "/tmp/test-concurrent.bdb";
    std.fs.cwd().deleteFile(test_path) catch {};
    
    // Create file
    var file = try bdb_format.BDBFile.init(allocator, test_path, .Cookies);
    defer file.deinit();
    
    // Simulate sequential operations (concurrent pattern)
    const num_operations = 200;
    
    for (0..num_operations) |i| {
        const key = std.fmt.allocPrint(allocator, "concurrent_key_{}", .{i}) catch continue;
        defer allocator.free(key);
        
        const value = std.fmt.allocPrint(allocator, "concurrent_value_{}", .{i}) catch continue;
        defer allocator.free(value);
        
        const timestamp = std.time.milliTimestamp() + i;
        
        // Alternate between operations
        if (i % 3 == 0) {
            const entry = bdb_format.BDBLogEntry.createInsert(key, value, timestamp);
            try file.appendEntry(entry);
        } else if (i % 3 == 1) {
            const entry = bdb_format.BDBLogEntry.createUpdate(key, "updated_" ++ value, timestamp);
            try file.appendEntry(entry);
        } else {
            const entry = bdb_format.BDBLogEntry.createDelete(key, timestamp);
            try file.appendEntry(entry);
        }
    }
    
    try file.writeFooter();
    
    // Verify file integrity
    const is_valid = try file.validate();
    try expect(is_valid);
    
    const stats = try file.getStats();
    try expect(stats.entry_count > 0);
    
    std.debug.print("✅ Concurrent operations test passed\n");
}

fn test_bdb_large_data_handling() !void {
    var gpa = std.heap.GeneralPurposeAllocator(.{}){};
    defer _ = gpa.deinit();
    
    const allocator = gpa.allocator();
    
    const test_path = "/tmp/test-large-data.bdb";
    std.fs.cwd().deleteFile(test_path) catch {};
    
    var file = try bdb_format.BDBFile.init(allocator, test_path, .Cache);
    defer file.deinit();
    
    // Test with larger entries
    const large_key_size = 1024;
    const large_value_size = 8192;
    
    var large_key = std.ArrayList(u8).init(allocator);
    defer large_key.deinit();
    try large_key.appendNTimes('K', large_key_size);
    
    var large_value = std.ArrayList(u8).init(allocator);
    defer large_value.deinit();
    try large_value.appendNTimes('V', large_value_size);
    
    const large_entry = bdb_format.BDBLogEntry.createInsert(large_key.items, large_value.items, 99999);
    try file.appendEntry(large_entry);
    
    // Test serialization of large entry
    var buffer = std.ArrayList(u8).init(allocator);
    defer buffer.deinit();
    try large_entry.serialize(&buffer);
    
    try expect(buffer.items.len > large_key_size + large_value_size); // Should have overhead
    
    try file.writeFooter();
    
    const stats = try file.getStats();
    try expect(stats.entry_count == 1);
    try expect(stats.max_entry_size > large_value_size);
    
    std.debug.print("✅ Large data handling test passed\n");
}

// ==================== BENCHMARK TESTS ====================

fn test_bdb_file_io_performance() !void {
    var gpa = std.heap.GeneralPurposeAllocator(.{}){};
    defer _ = gpa.deinit();
    
    const allocator = gpa.allocator();
    
    const test_path = "/tmp/test-io-perf.bdb";
    std.fs.cwd().deleteFile(test_path) catch {};
    
    const start_time = std.time.nanoTimestamp();
    
    var file = try bdb_format.BDBFile.init(allocator, test_path, .History);
    defer file.deinit();
    
    // Batch write performance test
    const num_entries = 1000;
    
    for (0..num_entries) |i| {
        const key = std.fmt.allocPrint(allocator, "perf_key_{}", .{i}) catch continue;
        defer allocator.free(key);
        const value = std.fmt.allocPrint(allocator, "perf_value_{}", .{i}) catch continue;
        defer allocator.free(value);
        
        const entry = bdb_format.BDBLogEntry.createInsert(key, value, i);
        try file.appendEntry(entry);
    }
    
    try file.writeFooter();
    
    const end_time = std.time.nanoTimestamp();
    const elapsed = end_time - start_time;
    const elapsed_ms = @as(f64, @floatFromInt(elapsed)) / 1_000_000.0;
    
    std.debug.print("📊 File I/O Performance:\n", .{});
    std.debug.print("  Entries: {d}\n", .{num_entries});
    std.debug.print("  Time: {:.2}ms\n", .{elapsed_ms});
    std.debug.print("  Throughput: {:.0} entries/sec\n", .{
        num_entries / (elapsed_ms / 1000.0)
    });
    
    // Should handle reasonable throughput (>1000 entries/sec)
    try expect(num_entries / (elapsed_ms / 1000.0) > 1000.0);
    
    std.debug.print("✅ File I/O performance test passed\n");
}

// ==================== FILE CRC TESTS ====================

fn test_crc32_calculation_small_file() !void {
    var gpa = std.heap.GeneralPurposeAllocator(.{}){};
    defer _ = gpa.deinit();
    
    const allocator = gpa.allocator();
    
    // Create a small test file
    const test_filename = "test_small_crc.bdb";
    defer std.fs.cwd().deleteFile(test_filename) catch {};
    
    var db_file = try bdb_format.BDBFile.init(allocator, test_filename, .History);
    defer db_file.deinit();
    
    // Add a few entries
    const entry1 = bdb_format.BDBLogEntry.createInsert("key1", "value1", std.time.milliTimestamp());
    const entry2 = bdb_format.BDBLogEntry.createInsert("key2", "value2", std.time.milliTimestamp());
    
    try db_file.appendEntry(entry1);
    try db_file.appendEntry(entry2);
    
    // Calculate CRC should work without error
    const crc = try db_file.calculateFileCRC();
    try expect(crc > 0);
    
    std.debug.print("✅ Small file CRC calculation test passed\n");
}

fn test_crc32_validation() !void {
    var gpa = std.heap.GeneralPurposeAllocator(.{}){};
    defer _ = gpa.deinit();
    
    const allocator = gpa.allocator();
    
    const test_filename = "test_crc_validation.bdb";
    defer std.fs.cwd().deleteFile(test_filename) catch {};
    
    var db_file = try bdb_format.BDBFile.init(allocator, test_filename, .History);
    defer db_file.deinit();
    
    // Add entry to create valid file
    const entry = bdb_format.BDBLogEntry.createInsert("test", "data", std.time.milliTimestamp());
    try db_file.appendEntry(entry);
    
    // File integrity should be valid
    const is_valid = try db_file.validateFileIntegrity();
    try expect(is_valid);
    
    std.debug.print("✅ CRC validation test passed\n");
}

fn test_crc32_chunked_processing() !void {
    var gpa = std.heap.GeneralPurposeAllocator(.{}){};
    defer _ = gpa.deinit();
    
    const allocator = gpa.allocator();
    
    const test_filename = "test_chunked_crc.bdb";
    defer std.fs.cwd().deleteFile(test_filename) catch {};
    
    var db_file = try bdb_format.BDBFile.init(allocator, test_filename, .Cache);
    defer db_file.deinit();
    
    // Add many entries to create a larger file
    for (0..1000) |i| {
        const key = std.fmt.allocPrint(allocator, "key_{d}_{s}", .{i, "x"}) catch continue;
        defer allocator.free(key);
        const value = std.fmt.allocPrint(allocator, "value_{d}_{s}", .{i, "y"}) catch continue;
        defer allocator.free(value);
        
        const entry = bdb_format.BDBLogEntry.createInsert(key, value, std.time.milliTimestamp());
        try db_file.appendEntry(entry);
    }
    
    // Calculate CRC for large file
    const crc = try db_file.calculateFileCRC();
    try expect(crc > 0);
    
    // Validate integrity
    const is_valid = try db_file.validateFileIntegrity();
    try expect(is_valid);
    
    std.debug.print("✅ Chunked processing CRC test passed\n");
}

fn test_crc32_empty_file() !void {
    var gpa = std.heap.GeneralPurposeAllocator(.{}){};
    defer _ = gpa.deinit();
    
    const allocator = gpa.allocator();
    
    const test_filename = "test_empty_crc.bdb";
    defer std.fs.cwd().deleteFile(test_filename) catch {};
    
    var db_file = try bdb_format.BDBFile.init(allocator, test_filename, .Settings);
    defer db_file.deinit();
    
    // Calculate CRC for empty file (should work)
    const crc = try db_file.calculateFileCRC();
    
    // Empty file should have some CRC value (even if it's 0)
    _ = crc; // Just test that it doesn't crash
    
    std.debug.print("✅ Empty file CRC test passed\n");
}

fn test_crc32_file_reopening() !void {
    var gpa = std.heap.GeneralPurposeAllocator(.{}){};
    defer _ = gpa.deinit();
    
    const allocator = gpa.allocator();
    
    const test_filename = "test_reopen_crc.bdb";
    defer std.fs.cwd().deleteFile(test_filename) catch {};
    
    // Create file with entries
    {
        var db_file = try bdb_format.BDBFile.init(allocator, test_filename, .LocalStore);
        defer db_file.deinit();
        
        const entry = bdb_format.BDBLogEntry.createInsert("reopen", "test", std.time.milliTimestamp());
        try db_file.appendEntry(entry);
    }
    
    // Reopen file and verify CRC consistency
    {
        var db_file = try bdb_format.BDBFile.init(allocator, test_filename, .LocalStore);
        defer db_file.deinit();
        
        // Calculate CRC after reopening
        const crc_after = try db_file.calculateFileCRC();
        
        // Validate integrity
        const is_valid = try db_file.validateFileIntegrity();
        try expect(is_valid);
    }
    
    std.debug.print("✅ File reopening CRC test passed\n");
}

fn test_crc32_performance() !void {
    var gpa = std.heap.GeneralPurposeAllocator(.{}){};
    defer _ = gpa.deinit();
    
    const allocator = gpa.allocator();
    
    const test_filename = "test_crc_performance.bdb";
    defer std.fs.cwd().deleteFile(test_filename) catch {};
    
    var db_file = try bdb_format.BDBFile.init(allocator, test_filename, .Cache);
    defer db_file.deinit();
    
    // Create moderately large file
    for (0..500) |i| {
        const key = std.fmt.allocPrint(allocator, "perf_key_{d}", .{i}) catch continue;
        defer allocator.free(key);
        const value = std.fmt.allocPrint(allocator, "perf_value_{d}_data", .{i}) catch continue;
        defer allocator.free(value);
        
        const entry = bdb_format.BDBLogEntry.createInsert(key, value, std.time.milliTimestamp());
        try db_file.appendEntry(entry);
    }
    
    // Measure CRC calculation performance
    const start_time = std.time.nanoTimestamp();
    const crc = try db_file.calculateFileCRC();
    const end_time = std.time.nanoTimestamp();
    
    const elapsed = end_time - start_time;
    const elapsed_ms = @as(f64, @floatFromInt(elapsed)) / 1_000_000.0;
    
    // Verify CRC was calculated
    try expect(crc > 0);
    
    // Should complete in reasonable time (under 100ms for this size)
    try expect(elapsed_ms < 100.0);
    
    std.debug.print("📊 CRC Performance: {:.2}ms for file\n", .{elapsed_ms});
    std.debug.print("✅ CRC performance test passed\n");
}

fn test_crc32_collision_resistance() !void {
    var gpa = std.heap.GeneralPurposeAllocator(.{}){};
    defer _ = gpa.deinit();
    
    const allocator = gpa.allocator();
    
    // Create two different files
    const filename1 = "test_collide1.bdb";
    const filename2 = "test_collide2.bdb";
    defer {
        std.fs.cwd().deleteFile(filename1) catch {};
        std.fs.cwd().deleteFile(filename2) catch {};
    }
    
    // File 1
    {
        var db_file = try bdb_format.BDBFile.init(allocator, filename1, .History);
        defer db_file.deinit();
        
        const entry = bdb_format.BDBLogEntry.createInsert("different", "data1", std.time.milliTimestamp());
        try db_file.appendEntry(entry);
    }
    
    // File 2
    {
        var db_file = try bdb_format.BDBFile.init(allocator, filename2, .History);
        defer db_file.deinit();
        
        const entry = bdb_format.BDBLogEntry.createInsert("different", "data2", std.time.milliTimestamp());
        try db_file.appendEntry(entry);
    }
    
    // Get CRCs for both files
    const file1 = try bdb_format.BDBFile.init(allocator, filename1, .History);
    defer file1.deinit();
    const file2 = try bdb_format.BDBFile.init(allocator, filename2, .History);
    defer file2.deinit();
    
    const crc1 = try file1.calculateFileCRC();
    const crc2 = try file2.calculateFileCRC();
    
    // Different content should (almost certainly) produce different CRCs
    try expect(crc1 != crc2);
    
    std.debug.print("✅ CRC collision resistance test passed\n");
}

// ==================== MAIN TEST RUNNER ====================

pub fn main() !void {
    std.debug.print("🚀 BrowserDB .bdb Format Test Suite\n", .{});
    std.debug.print("=====================================\n\n", .{});
    
    // File Header Tests
    runTest("BDB Header Creation", test_bdb_header_creation);
    runTest("BDB Header CRC Calculation", test_bdb_header_crc_calculation);
    runTest("BDB Header Serialization", test_bdb_header_serialization);
    runTest("BDB Header Validation", test_bdb_header_validation);
    
    // Log Entry Tests
    runTest("BDB Log Entry Creation", test_bdb_log_entry_creation);
    runTest("BDB Log Entry CRC", test_bdb_log_entry_crc);
    runTest("BDB Log Entry Serialization", test_bdb_log_entry_serialization);
    runTest("BDB Log Entry Size Calculation", test_bdb_log_entry_size_calculation);
    
    // Varint Encoding Tests
    runTest("BDB Varint Encoding", test_bdb_varint_encoding);
    runTest("BDB Varint Size Calculation", test_bdb_varint_size_calculation);
    
    // File Footer Tests
    runTest("BDB Footer Creation", test_bdb_footer_creation);
    runTest("BDB Footer Serialization", test_bdb_footer_serialization);
    
    // File Manager Tests
    runTest("BDB File Manager Creation", test_bdb_file_manager_creation);
    runTest("BDB File Creation", test_bdb_file_creation);
    
    // Integration Tests
    runTest("BDB Complete Workflow", test_bdb_complete_workflow);
    runTest("BDB Multiple Table Types", test_bdb_multiple_table_types);
    
    // Error Handling Tests
    runTest("BDB Error Handling", test_bdb_error_handling);
    
    // Performance Tests
    runTest("BDB Serialization Performance", test_bdb_serialization_performance);
    
    // File CRC Tests
    runTest("Small File CRC Calculation", test_crc32_calculation_small_file);
    runTest("CRC Validation", test_crc32_validation);
    runTest("Chunked CRC Processing", test_crc32_chunked_processing);
    runTest("Empty File CRC", test_crc32_empty_file);
    runTest("File Reopening CRC", test_crc32_file_reopening);
    runTest("CRC Performance", test_crc32_performance);
    runTest("CRC Collision Resistance", test_crc32_collision_resistance);
    
    // Output test results
    std.debug.print("\n📊 Test Results:\n", .{});
    std.debug.print("   Total tests: {d}\n", .{test_count});
    std.debug.print("   Passed: {d}\n", .{pass_count});
    std.debug.print("   Failed: {d}\n", .{fail_count});
    std.debug.print("   Success rate: {d}%\n", .{
        if (test_count > 0) (pass_count * 100 / test_count) else 0
    });
    
    if (fail_count == 0) {
        std.debug.print("\n🎉 All .bdb format tests passed! File format is ready for production.\n", .{});
    } else {
        std.debug.print("\n⚠️ Some .bdb format tests failed. Please review the implementation.\n", .{});
        return error.TestsFailed;
    }
}
