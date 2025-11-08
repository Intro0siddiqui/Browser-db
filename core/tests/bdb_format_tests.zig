//! BrowserDB File Format (.bdb) Tests
//!
//! This test suite provides comprehensive validation of the .bdb file format,
//! ensuring data integrity, corruption resistance, and cross-platform compatibility.
//! Tests cover:
//! - Header validation and magic number verification
//! - SSTable, WAL, and Index file parsing
//! - Entry serialization and deserialization
//! - Checksum and CRC32 validation
//! - Metadata handling
//! - Error recovery and corruption detection

const std = @import("std");
const testing = std.testing;
const bdb = @import("../src/core/bdb_format.zig");

// Test allocator for controlled memory management
const TestAllocator = struct {
    backing: std.heap.GeneralPurposeAllocator(.{}),
    
    pub fn init() @This() {
        return .{ .backing = std.heap.GeneralPurposeAllocator(.{}){} };
    }

    pub fn allocator(self: *@This()) std.mem.Allocator {
        return self.backing.allocator();
    }

    pub fn deinit(self: *@This()) void {
        _ = self.backing.deinit();
    }
};

// ==================== BDBKey & BDBValue Tests ====================

test "BDBKey - Creation and Validation" {
    const key_data = "test-key-123";
    const bdb_key = try bdb.BDBKey.fromString(key_data);
    
    try testing.expect(std.mem.eql(u8, &bdb_key.data, key_data));
    try testing.expectEqual(@as(u16, key_data.len), bdb_key.len);
}

test "BDBValue - Creation and Validation" {
    const value_data = "test-value-with-some-data";
    const bdb_value = try bdb.BDBValue.fromString(value_data);
    
    try testing.expect(std.mem.eql(u8, &bdb_value.data, value_data));
    try testing.expectEqual(@as(u32, value_data.len), bdb_value.len);
}

test "BDBKey - Empty and Large Keys" {
    // Test empty key
    const empty_key = try bdb.BDBKey.fromString("");
    try testing.expectEqual(@as(u16, 0), empty_key.len);
    
    // Test large key (up to max u16)
    var large_key_data = std.ArrayList(u8).init(std.testing.allocator);
    defer large_key_data.deinit();
    try large_key_data.appendNTimes('A', bdb.BDBKey.MAX_LEN);
    
    const large_key = try bdb.BDBKey.fromString(large_key_data.items);
    try testing.expectEqual(bdb.BDBKey.MAX_LEN, large_key.len);
    
    // Test oversized key (should fail)
    try large_key_data.append('A');
    try testing.expectError(error.KeyTooLarge, bdb.BDBKey.fromString(large_key_data.items));
}

// ==================== BDBHeader Tests ====================

test "BDBHeader - Default Initialization and Validation" {
    var header = bdb.BDBHeader.init(.SSTable);
    
    try header.validate();
    
    try testing.expectEqualSlice(u8, bdb.BDBHeader.MAGIC, &header.magic);
    try testing.expectEqual(bdb.BDBHeader.VERSION, header.version);
    try testing.expectEqual(@as(u64, 0), header.entry_count);
    try testing.expectEqual(bdb.TableType.SSTable, header.table_type);
}

test "BDBHeader - Serialization and Deserialization" {
    var original_header = bdb.BDBHeader.init(.WriteAheadLog);
    original_header.entry_count = 12345;
    original_header.compression_type = .Snappy;
    original_header.encryption_type = .AES_256_GCM;
    
    // Calculate checksum before serialization
    original_header.header_checksum = original_header.calculateChecksum();
    
    var buffer = std.ArrayList(u8).init(std.testing.allocator);
    defer buffer.deinit();
    
    const writer = buffer.writer();
    try original_header.serialize(writer);
    
    // Deserialize
    var reader = std.io.fixedBufferStream(buffer.items).reader();
    const deserialized_header = try bdb.BDBHeader.deserialize(reader);
    
    // Validate
    try deserialized_header.validate();
    
    try testing.expectEqual(original_header.version, deserialized_header.version);
    try testing.expectEqual(original_header.entry_count, deserialized_header.entry_count);
    try testing.expectEqual(original_header.table_type, deserialized_header.table_type);
    try testing.expectEqual(original_header.compression_type, deserialized_header.compression_type);
    try testing.expectEqual(original_header.encryption_type, deserialized_header.encryption_type);
    try testing.expectEqual(original_header.header_checksum, deserialized_header.header_checksum);
}

test "BDBHeader - Corruption Detection (Magic Number)" {
    var header = bdb.BDBHeader.init(.SSTable);
    header.magic[0] = 'X'; // Corrupt magic number
    
    var buffer = std.ArrayList(u8).init(std.testing.allocator);
    defer buffer.deinit();
    
    const writer = buffer.writer();
    try header.serialize(writer);
    
    var reader = std.io.fixedBufferStream(buffer.items).reader();
    try testing.expectError(error.InvalidMagicNumber, bdb.BDBHeader.deserialize(reader));
}

test "BDBHeader - Corruption Detection (Checksum)" {
    var header = bdb.BDBHeader.init(.SSTable);
    header.header_checksum = 123; // Incorrect checksum
    
    var buffer = std.ArrayList(u8).init(std.testing.allocator);
    defer buffer.deinit();
    
    const writer = buffer.writer();
    try header.serialize(writer);
    
    var reader = std.io.fixedBufferStream(buffer.items).reader();
    const deserialized_header = try bdb.BDBHeader.deserialize(reader);
    
    try testing.expectError(error.InvalidHeaderChecksum, deserialized_header.validate());
}

// ==================== BDBLogEntry Tests ====================

test "BDBLogEntry - Insert Entry Serialization and Deserialization" {
    const allocator = std.testing.allocator;
    
    // Create entry
    const key = try bdb.BDBKey.fromString("log-key");
    const value = try bdb.BDBValue.fromString("log-value");
    const timestamp = std.time.timestamp();
    
    var original_entry = bdb.BDBLogEntry.create(.Insert, key, value, timestamp);
    original_entry.updateChecksum();
    
    // Serialize
    var buffer = std.ArrayList(u8).init(allocator);
    defer buffer.deinit();
    const writer = buffer.writer();
    try original_entry.serialize(writer);
    
    // Deserialize
    var reader = std.io.fixedBufferStream(buffer.items).reader();
    const deserialized_entry = try bdb.BDBLogEntry.deserialize(reader, allocator);
    defer deserialized_entry.deinit(allocator);
    
    // Validate
    try testing.expectEqual(original_entry.entry_type, deserialized_entry.entry_type);
    try testing.expectEqual(original_entry.timestamp, deserialized_entry.timestamp);
    try testing.expect(std.mem.eql(u8, &original_entry.key.data, &deserialized_entry.key.data));
    try testing.expect(std.mem.eql(u8, &original_entry.value.data, &deserialized_entry.value.data));
    try testing.expectEqual(original_entry.checksum, deserialized_entry.checksum);
}

test "BDBLogEntry - Delete Entry Serialization and Deserialization" {
    const allocator = std.testing.allocator;
    
    // Create delete entry (value is empty)
    const key = try bdb.BDBKey.fromString("delete-key");
    const value = try bdb.BDBValue.fromString("");
    const timestamp = std.time.timestamp();
    
    var original_entry = bdb.BDBLogEntry.create(.Delete, key, value, timestamp);
    original_entry.updateChecksum();
    
    // Serialize
    var buffer = std.ArrayList(u8).init(allocator);
    defer buffer.deinit();
    const writer = buffer.writer();
    try original_entry.serialize(writer);
    
    // Deserialize
    var reader = std.io.fixedBufferStream(buffer.items).reader();
    const deserialized_entry = try bdb.BDBLogEntry.deserialize(reader, allocator);
    defer deserialized_entry.deinit(allocator);
    
    // Validate
    try testing.expectEqual(bdb.BDBLogEntry.EntryType.Delete, deserialized_entry.entry_type);
    try testing.expect(std.mem.eql(u8, &original_entry.key.data, &deserialized_entry.key.data));
    try testing.expectEqual(@as(u32, 0), deserialized_entry.value.len);
    try testing.expectEqual(original_entry.checksum, deserialized_entry.checksum);
}

test "BDBLogEntry - Corruption Detection (Checksum)" {
    const allocator = std.testing.allocator;
    
    const key = try bdb.BDBKey.fromString("log-key");
    const value = try bdb.BDBValue.fromString("log-value");
    var entry = bdb.BDBLogEntry.create(.Insert, key, value, 12345);
    entry.checksum = 999; // Corrupt checksum
    
    var buffer = std.ArrayList(u8).init(allocator);
    defer buffer.deinit();
    const writer = buffer.writer();
    try entry.serialize(writer);
    
    var reader = std.io.fixedBufferStream(buffer.items).reader();
    const deserialized = try bdb.BDBLogEntry.deserialize(reader, allocator);
    defer deserialized.deinit(allocator);
    
    try testing.expectError(error.InvalidEntryChecksum, deserialized.validate());
}

// ==================== Full .bdb File Integration Tests ====================

test "Full .bdb File - Write and Read SSTable" {
    const allocator = std.testing.allocator;
    const test_path = "/tmp/test_sstable.bdb";
    
    // Phase 1: Write the file
    {
        var file = try std.fs.cwd().createFile(test_path, .{});
        defer file.close();
        
        const writer = file.writer();
        
        // Write header
        var header = bdb.BDBHeader.init(.SSTable);
        header.entry_count = 2;
        header.header_checksum = header.calculateChecksum();
        try header.serialize(writer);
        
        // Write entries
        const key1 = try bdb.BDBKey.fromString("key1");
        const value1 = try bdb.BDBValue.fromString("value1");
        var entry1 = bdb.BDBLogEntry.create(.Insert, key1, value1, 1);
        entry1.updateChecksum();
        try entry1.serialize(writer);
        
        const key2 = try bdb.BDBKey.fromString("key2");
        const value2 = try bdb.BDBValue.fromString("value2");
        var entry2 = bdb.BDBLogEntry.create(.Insert, key2, value2, 2);
        entry2.updateChecksum();
        try entry2.serialize(writer);
    }
    
    // Phase 2: Read and verify the file
    {
        var file = try std.fs.cwd().openFile(test_path, .{});
        defer file.close();
        
        const reader = file.reader();
        
        // Read and validate header
        const header = try bdb.BDBHeader.deserialize(reader);
        try header.validate();
        try testing.expectEqual(@as(u64, 2), header.entry_count);
        try testing.expectEqual(bdb.TableType.SSTable, header.table_type);
        
        // Read and validate entries
        const entry1 = try bdb.BDBLogEntry.deserialize(reader, allocator);
        defer entry1.deinit(allocator);
        try entry1.validate();
        try testing.expect(std.mem.eql(u8, &entry1.key.data, "key1"));
        try testing.expect(std.mem.eql(u8, &entry1.value.data, "value1"));
        
        const entry2 = try bdb.BDBLogEntry.deserialize(reader, allocator);
        defer entry2.deinit(allocator);
        try entry2.validate();
        try testing.expect(std.mem.eql(u8, &entry2.key.data, "key2"));
        try testing.expect(std.mem.eql(u8, &entry2.value.data, "value2"));
    }
    
    std.fs.cwd().deleteFile(test_path) catch {};
}

test "Full .bdb File - Write and Read WAL" {
    const allocator = std.testing.allocator;
    const test_path = "/tmp/test_wal.bdb";
    
    // Phase 1: Write WAL file
    {
        var file = try std.fs.cwd().createFile(test_path, .{});
        defer file.close();
        
        const writer = file.writer();
        
        // Write header
        var header = bdb.BDBHeader.init(.WriteAheadLog);
        header.entry_count = 3;
        header.header_checksum = header.calculateChecksum();
        try header.serialize(writer);
        
        // Write insert entry
        var entry1 = bdb.BDBLogEntry.create(.Insert, try bdb.BDBKey.fromString("key1"), try bdb.BDBValue.fromString("value1"), 1);
        entry1.updateChecksum();
        try entry1.serialize(writer);
        
        // Write update entry
        var entry2 = bdb.BDBLogEntry.create(.Update, try bdb.BDBKey.fromString("key1"), try bdb.BDBValue.fromString("value2"), 2);
        entry2.updateChecksum();
        try entry2.serialize(writer);
        
        // Write delete entry
        var entry3 = bdb.BDBLogEntry.create(.Delete, try bdb.BDBKey.fromString("key2"), try bdb.BDBValue.fromString(""), 3);
        entry3.updateChecksum();
        try entry3.serialize(writer);
    }
    
    // Phase 2: Read and verify WAL file
    {
        var file = try std.fs.cwd().openFile(test_path, .{});
        defer file.close();
        
        const reader = file.reader();
        
        // Read header
        const header = try bdb.BDBHeader.deserialize(reader);
        try header.validate();
        try testing.expectEqual(@as(u64, 3), header.entry_count);
        
        // Read entries
        const entry1 = try bdb.BDBLogEntry.deserialize(reader, allocator);
        defer entry1.deinit(allocator);
        try entry1.validate();
        try testing.expectEqual(bdb.BDBLogEntry.EntryType.Insert, entry1.entry_type);

        const entry2 = try bdb.BDBLogEntry.deserialize(reader, allocator);
        defer entry2.deinit(allocator);
        try entry2.validate();
        try testing.expectEqual(bdb.BDBLogEntry.EntryType.Update, entry2.entry_type);
        
        const entry3 = try bdb.BDBLogEntry.deserialize(reader, allocator);
        defer entry3.deinit(allocator);
        try entry3.validate();
        try testing.expectEqual(bdb.BDBLogEntry.EntryType.Delete, entry3.entry_type);
    }
    
    std.fs.cwd().deleteFile(test_path) catch {};
}

// ==================== Metadata Tests ====================

test "BDBMetadata - Serialization and Deserialization" {
    const allocator = std.testing.allocator;
    
    // Create metadata
    var original_meta = bdb.BDBMetadata.init();
    original_meta.min_timestamp = 1000;
    original_meta.max_timestamp = 5000;
    original_meta.bloom_filter_size = 1024;
    original_meta.bloom_filter_hashes = 5;
    
    // Serialize
    var buffer = std.ArrayList(u8).init(allocator);
    defer buffer.deinit();
    const writer = buffer.writer();
    try original_meta.serialize(writer);
    
    // Deserialize
    var reader = std.io.fixedBufferStream(buffer.items).reader();
    const deserialized_meta = try bdb.BDBMetadata.deserialize(reader);
    
    // Validate
    try testing.expectEqual(original_meta.min_timestamp, deserialized_meta.min_timestamp);
    try testing.expectEqual(original_meta.max_timestamp, deserialized_meta.max_timestamp);
    try testing.expectEqual(original_meta.bloom_filter_size, deserialized_meta.bloom_filter_size);
    try testing.expectEqual(original_meta.bloom_filter_hashes, deserialized_meta.bloom_filter_hashes);
}

// ==================== Edge Case and Regression Tests ====================

test "Edge Case - Reading from Empty File" {
    const test_path = "/tmp/empty.bdb";
    var file = try std.fs.cwd().createFile(test_path, .{});
    file.close();
    
    var open_file = try std.fs.cwd().openFile(test_path, .{});
    defer open_file.close();
    
    const reader = open_file.reader();
    
    // Should fail gracefully (e.g., with EndOfStream)
    try testing.expectError(error.EndOfStream, bdb.BDBHeader.deserialize(reader));
    
    std.fs.cwd().deleteFile(test_path) catch {};
}

test "Edge Case - Handling of Zero-Byte Value" {
    const allocator = std.testing.allocator;
    
    const key = try bdb.BDBKey.fromString("zero-value-key");
    const value = try bdb.BDBValue.fromString(""); // Zero-byte value
    
    var entry = bdb.BDBLogEntry.create(.Insert, key, value, 999);
    entry.updateChecksum();
    
    var buffer = std.ArrayList(u8).init(allocator);
    defer buffer.deinit();
    const writer = buffer.writer();
    try entry.serialize(writer);
    
    var reader = std.io.fixedBufferStream(buffer.items).reader();
    const deserialized = try bdb.BDBLogEntry.deserialize(reader, allocator);
    defer deserialized.deinit(allocator);
    
    try deserialized.validate();
    try testing.expectEqual(@as(u32, 0), deserialized.value.len);
}
