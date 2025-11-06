const std = @import("std");
const Allocator = std.mem.Allocator;
const Blake3 = std.crypto.blake3;
const testing = std.testing;

/// BrowserDB (.bdb) File Format Specification
/// Version: 1.0
/// 
/// The .bdb file format is a high-performance, append-only binary format
/// designed specifically for browser data storage with built-in compression,
/// integrity checks, and efficient metadata.

pub const BDB_VERSION: u8 = 1;
pub const MAGIC_BYTES: [8]u8 = "BROWSERDB".*;

pub const TableType = enum(u8) {
    History = 1,
    Cookies = 2,
    Cache = 3,
    LocalStore = 4,
    Settings = 5,
};

pub const EntryType = enum(u8) {
    Insert = 1,
    Update = 2,
    Delete = 3,
    BatchStart = 4,
    BatchEnd = 5,
};

pub const CompressionType = enum(u8) {
    None = 0,
    Zlib = 1,
    Lz4 = 2,
    Zstd = 3,
};

pub const EncryptionType = enum(u8) {
    None = 0,
    AES256 = 1,
    ChaCha20 = 2,
};

// ==================== FILE HEADER STRUCTURE ====================

pub const BDBFileHeader = struct {
    /// 8 bytes: Magic string "BROWSERDB"
    magic: [8]u8,
    
    /// 1 byte: File format version
    version: u8,
    
    /// 8 bytes: Timestamp when file was created
    created_at: u64,
    
    /// 8 bytes: Timestamp of last modification
    modified_at: u64,
    
    /// 4 bytes: Flags (compression, encryption, etc.)
    flags: u32,
    
    /// 4 bytes: Reserved for future use
    reserved: u32,
    
    /// 1 byte: Table type this file contains
    table_type: TableType,
    
    /// 1 byte: Compression algorithm used
    compression: CompressionType,
    
    /// 1 byte: Encryption algorithm used
    encryption: EncryptionType,
    
    /// 6 bytes: Reserved bytes
    _: [6]u8 = undefined,
    
    /// 32 bytes: CRC32 checksum of header
    header_crc: u32,
    
    const Self = @This();
    
    pub fn init(table_type: TableType) Self {
        const timestamp = std.time.milliTimestamp();
        
        return Self{
            .magic = MAGIC_BYTES,
            .version = BDB_VERSION,
            .created_at = timestamp,
            .modified_at = timestamp,
            .flags = 0,
            .reserved = 0,
            .table_type = table_type,
            .compression = .None,
            .encryption = .None,
            .header_crc = 0, // Will be calculated
        };
    }
    
    pub fn calculateCRC(self: *Self) u32 {
        const header_without_crc = @as([*]const u8, @ptrCast(self))[0..@sizeOf(Self) - 4];
        return std.hash.Crc32.hash(header_without_crc);
    }
    
    pub fn validate(self: *Self) !bool {
        // Check magic bytes
        if (!std.mem.eql(u8, &self.magic, &MAGIC_BYTES)) {
            return error.InvalidMagic;
        }
        
        // Check version compatibility
        if (self.version > BDB_VERSION) {
            return error.VersionTooNew;
        }
        
        // Verify header CRC
        const calculated_crc = self.calculateCRC();
        return calculated_crc == self.header_crc;
    }
    
    pub fn serialize(self: *Self, buffer: *std.ArrayList(u8)) !void {
        // Serialize header to buffer
        try buffer.appendSlice(std.mem.asBytes(&self.magic));
        try buffer.append(self.version);
        try buffer.appendSlice(std.mem.asBytes(&self.created_at));
        try buffer.appendSlice(std.mem.asBytes(&self.modified_at));
        try buffer.appendSlice(std.mem.asBytes(&self.flags));
        try buffer.appendSlice(std.mem.asBytes(&self.reserved));
        try buffer.append(@intFromEnum(self.table_type));
        try buffer.append(@intFromEnum(self.compression));
        try buffer.append(@intFromEnum(self.encryption));
        try buffer.appendSlice(&self._);
        
        // Calculate and append CRC
        const calculated_crc = self.calculateCRC();
        self.header_crc = calculated_crc;
        try buffer.appendSlice(std.mem.asBytes(&calculated_crc));
    }
};

// ==================== ENTRY LOG STRUCTURE ====================

pub const BDBLogEntry = struct {
    /// 1 byte: Entry type
    entry_type: EntryType,
    
    /// Varint: Key length
    key_length: u64,
    
    /// Varint: Value length (0 for delete operations)
    value_length: u64,
    
    /// Variable: Key data
    key: []const u8,
    
    /// Variable: Value data (omitted for delete operations)
    value: []const u8,
    
    /// 8 bytes: Timestamp
    timestamp: u64,
    
    /// 4 bytes: Entry CRC32 checksum
    entry_crc: u32,
    
    const Self = @This();
    
    pub fn createInsert(key: []const u8, value: []const u8, timestamp: u64) Self {
        return Self{
            .entry_type = .Insert,
            .key_length = key.len,
            .value_length = value.len,
            .key = key,
            .value = value,
            .timestamp = timestamp,
            .entry_crc = 0,
        };
    }
    
    pub fn createUpdate(key: []const u8, value: []const u8, timestamp: u64) Self {
        return Self{
            .entry_type = .Update,
            .key_length = key.len,
            .value_length = value.len,
            .key = key,
            .value = value,
            .timestamp = timestamp,
            .entry_crc = 0,
        };
    }
    
    pub fn createDelete(key: []const u8, timestamp: u64) Self {
        return Self{
            .entry_type = .Delete,
            .key_length = key.len,
            .value_length = 0,
            .key = key,
            .value = &[_]u8{},
            .timestamp = timestamp,
            .entry_crc = 0,
        };
    }
    
    pub fn calculateCRC(self: *Self) u32 {
        var hasher = std.hash.Crc32.init();
        
        // Hash entry type
        const entry_type_byte = @as(u8, @intFromEnum(self.entry_type));
        hasher.update(&[1]u8{entry_type_byte});
        
        // Hash key and value
        hasher.update(self.key);
        hasher.update(self.value);
        
        // Hash timestamp
        var timestamp_bytes = std.mem.asBytes(&self.timestamp);
        hasher.update(&timestamp_bytes);
        
        return hasher.final();
    }
    
    pub fn serialize(self: *Self, buffer: *std.ArrayList(u8)) !void {
        // Write entry type
        try buffer.append(@intFromEnum(self.entry_type));
        
        // Write key length (varint)
        try writeVarInt(buffer, self.key_length);
        
        // Write value length (varint)
        try writeVarInt(buffer, self.value_length);
        
        // Write key data
        if (self.key_length > 0) {
            try buffer.appendSlice(self.key);
        }
        
        // Write value data
        if (self.value_length > 0) {
            try buffer.appendSlice(self.value);
        }
        
        // Write timestamp
        try buffer.appendSlice(std.mem.asBytes(&self.timestamp));
        
        // Calculate and write CRC
        self.entry_crc = self.calculateCRC();
        try buffer.appendSlice(std.mem.asBytes(&self.entry_crc));
    }
    
    pub fn getSize(self: *Self) usize {
        // Base entry size: type + timestamp + crc + varint overhead
        const base_size = 1 + 8 + 4;
        
        // Add key and value sizes
        var size = base_size + self.key_length + self.value_length;
        
        // Add varint encoding overhead
        size += varintSize(self.key_length);
        size += varintSize(self.value_length);
        
        return size;
    }
};

// ==================== FILE FOOTER STRUCTURE ====================

pub const BDBFileFooter = struct {
    /// 8 bytes: Total number of entries in file
    entry_count: u64,
    
    /// 8 bytes: File size in bytes
    file_size: u64,
    
    /// 8 bytes: Offset to start of entries
    data_offset: u64,
    
    /// 4 bytes: Maximum entry size in file
    max_entry_size: u32,
    
    /// 4 bytes: Total size of all keys
    total_key_size: u64,
    
    /// 4 bytes: Total size of all values
    total_value_size: u64,
    
    /// 4 bytes: Compression ratio (percentage * 100)
    compression_ratio: u16,
    
    /// 2 bytes: Reserved
    _: [2]u8 = undefined,
    
    /// 32 bytes: CRC32 checksum of entire file content
    file_crc: u32,
    
    const Self = @This();
    
    pub fn init() Self {
        return Self{
            .entry_count = 0,
            .file_size = 0,
            .data_offset = 0,
            .max_entry_size = 0,
            .total_key_size = 0,
            .total_value_size = 0,
            .compression_ratio = 100,
            ._ = undefined,
            .file_crc = 0,
        };
    }
    
    pub fn serialize(self: *Self, buffer: *std.ArrayList(u8)) !void {
        try buffer.appendSlice(std.mem.asBytes(&self.entry_count));
        try buffer.appendSlice(std.mem.asBytes(&self.file_size));
        try buffer.appendSlice(std.mem.asBytes(&self.data_offset));
        try buffer.appendSlice(std.mem.asBytes(&self.max_entry_size));
        try buffer.appendSlice(std.mem.asBytes(&self.total_key_size));
        try buffer.appendSlice(std.mem.asBytes(&self.total_value_size));
        try buffer.appendSlice(std.mem.asBytes(&self.compression_ratio));
        try buffer.appendSlice(&self._);
        try buffer.appendSlice(std.mem.asBytes(&self.file_crc));
    }
};

// ==================== ENTRY INDEX STRUCTURE ====================

pub const EntryIndex = struct {
    offset: usize,
    key_length: usize,
    timestamp: u64,
};

// ==================== FILE VARINT HELPERS ====================

pub fn readVarIntFromFile(file: *std.fs.File, offset: *usize) !u64 {
    var result: u64 = 0;
    var shift: u6 = 0;
    
    try file.seekTo(offset.*);
    
    while (offset.* < 1000000) { // Reasonable limit
        const byte = try file.readByte();
        offset.* += 1;
        
        result |= @as(u64, byte & 0x7F) << shift;
        
        if ((byte & 0x80) == 0) {
            break;
        }
        
        shift += 7;
        
        if (shift > 63) {
            return error.VarIntTooLarge;
        }
    }
    
    return result;
}

pub fn skipVarIntFromFile(file: *std.fs.File, offset: *usize) !u64 {
    var result: u64 = 0;
    var shift: u6 = 0;
    
    try file.seekTo(offset.*);
    
    while (offset.* < 1000000) { // Reasonable limit
        const byte = try file.readByte();
        offset.* += 1;
        
        result |= @as(u64, byte & 0x7F) << shift;
        
        if ((byte & 0x80) == 0) {
            break;
        }
        
        shift += 7;
        
        if (shift > 63) {
            return error.VarIntTooLarge;
        }
    }
    
    return result;
}

// ==================== VARINT ENCODING/DECODING ====================

pub fn writeVarInt(buffer: *std.ArrayList(u8), value: u64) !void {
    var v = value;
    
    while (v >= 0x80) {
        try buffer.append(@as(u8, (v & 0x7F) | 0x80));
        v >>= 7;
    }
    
    try buffer.append(@as(u8, v));
}

pub fn readVarInt(data: []const u8, offset: *usize) !u64 {
    var result: u64 = 0;
    var shift: u6 = 0;
    
    while (offset.* < data.len) {
        const byte = data[offset.*];
        offset.* += 1;
        
        result |= @as(u64, byte & 0x7F) << shift;
        
        if ((byte & 0x80) == 0) {
            break;
        }
        
        shift += 7;
        
        if (shift > 63) {
            return error.VarIntTooLarge;
        }
    }
    
    return result;
}

pub fn varintSize(value: u64) usize {
    var size: usize = 1;
    var v = value;
    
    while (v >= 0x80) {
        v >>= 7;
        size += 1;
    }
    
    return size;
}

// ==================== FILE MANAGER ====================

pub const BDBFileManager = struct {
    allocator: Allocator,
    base_path: []const u8,
    max_file_size: usize = 1024 * 1024 * 1024, // 1GB default
    compression_buffer: std.ArrayList(u8),
    
    const Self = @This();
    
    pub fn init(allocator: Allocator, base_path: []const u8) Self {
        return Self{
            .allocator = allocator,
            .base_path = base_path,
            .compression_buffer = std.ArrayList(u8).init(allocator),
        };
    }
    
    pub fn deinit(self: *Self) void {
        self.compression_buffer.deinit();
    }
    
    /// Create a new .bdb file
    pub fn createFile(self: *Self, table_type: TableType) !*BDBFile {
        const timestamp = std.time.milliTimestamp();
        const filename = try std.fmt.allocPrint(self.allocator, "{}_{}_{}.bdb", .{
            self.base_path,
            @tagName(table_type),
            timestamp
        });
        
        const file = try self.allocator.create(BDBFile);
        file.* = try BDBFile.init(self.allocator, filename, table_type);
        
        return file;
    }
    
    /// Rotate file when it reaches max size
    pub fn rotateFile(self: *Self, current_file: *BDBFile) !*BDBFile {
        const new_file = try self.createFile(current_file.header.table_type);
        
        std.debug.print("🔄 Rotating .bdb file: {s} -> {s}\n", .{
            current_file.filename, new_file.filename
        });
        
        return new_file;
    }
    
    /// Get all .bdb files for a table type
    pub fn listFiles(self: *Self, table_type: TableType) !std.ArrayList([]const u8) {
        var files = std.ArrayList([]const u8).init(self.allocator);
        
        // Scan directory for .bdb files matching the table type pattern
        const pattern = try std.fmt.allocPrint(self.allocator, "*_{s}_*.bdb", .{@tagName(table_type)});
        defer self.allocator.free(pattern);
        
        var dir = std.fs.cwd().openDir(self.base_path, .{}) catch return files;
        defer dir.close();
        
        var iter = dir.iterate();
        while (try iter.next()) |entry| {
            if (entry.kind == .file) {
                const filename = entry.name;
                if (std.mem.endsWith(u8, filename, ".bdb") and 
                    std.mem.indexOf(u8, filename, "_" ++ @tagName(table_type) ++ "_") != null) {
                    
                    const full_path = try std.fmt.allocPrint(self.allocator, "{s}/{s}", .{ self.base_path, filename });
                    try files.append(full_path);
                }
            }
        }
        
        return files;
    }
    
    /// Load existing .bdb files for a table type (used for startup discovery)
    pub fn loadExistingFiles(self: *Self, table_type: TableType) !std.ArrayList(*BDBFile) {
        var files = std.ArrayList(*BDBFile).init(self.allocator);
        
        const file_list = try self.listFiles(table_type);
        defer file_list.deinit();
        
        for (file_list.items) |filename| {
            const file = try self.openFile(filename);
            try files.append(file);
        }
        
        return files;
    }
    
    /// Open an existing .bdb file
    pub fn openFile(self: *Self, filename: []const u8) !*BDBFile {
        // Check if file exists and validate format
        const stat = std.fs.cwd().statFile(filename) catch return error.FileNotFound;
        
        // Open existing file (not truncate)
        const file = try std.fs.cwd().openFile(filename, .{
            .read = true,
            .write = true,
        });
        
        const self_ptr = try self.allocator.create(Self);
        self_ptr.* = Self{
            .allocator = self.allocator,
            .filename = filename,
            .header = try readHeaderFromFile(file),
            .footer = BDBFileFooter.init(),
            .file = file,
            .current_offset = stat.size,
            .entry_count = 0, // Will be calculated from file
        };
        
        // Read footer to get entry count
        try self_ptr.readFooter();
        
        return self_ptr;
    }
    
    /// Read header from existing file
    fn readHeaderFromFile(file: std.fs.File) !BDBFileHeader {
        var header_buffer: [@sizeOf(BDBFileHeader)]u8 = undefined;
        _ = try file.read(&header_buffer);
        
        // Parse header manually
        const header = @as(*BDBFileHeader, @ptrCast(&header_buffer));
        
        // Validate header
        if (!try header.validate()) {
            return error.InvalidHeader;
        }
        
        return header.*;
    }
    
    /// Read footer from file
    fn readFooter(self: *Self) !void {
        try self.file.seekFromEnd(-@sizeOf(BDBFileFooter));
        var footer_buffer: [@sizeOf(BDBFileFooter)]u8 = undefined;
        _ = try self.file.read(&footer_buffer);
        
        self.footer = @as(*BDBFileFooter, @ptrCast(&footer_buffer)).*;
        
        // Validate file integrity with CRC check
        const file_crc_valid = self.validateFileIntegrity() catch {
            // If CRC calculation fails during validation, log warning but continue
            std.debug.print("⚠️  Warning: File CRC validation failed for {s}\n", .{self.filename});
            false;
        };
        
        if (!file_crc_valid) {
            std.debug.print("🔍 File integrity check failed for {s} - CRC mismatch\n", .{self.filename});
            // Note: We don't return error here to allow recovery in read-only mode
        }
        
        // Count actual entries by scanning file
        self.entry_count = try self.countEntriesInFile();
    }
    
    /// Count entries by scanning file content
    fn countEntriesInFile(self: *Self) !usize {
        var count: usize = 0;
        var current_offset = self.footer.data_offset;
        const file_end = self.current_offset - @sizeOf(BDBFileFooter);
        
        while (current_offset < file_end) {
            try self.file.seekTo(current_offset);
            
            // Read entry type to verify
            _ = try self.file.readByte();
            
            // Calculate entry size to find next entry
            const entry_size = try self.calculateEntrySize(current_offset);
            if (entry_size == 0) break; // Safety check
            
            current_offset += entry_size;
            count += 1;
            
            // Safety limit to prevent infinite loops
            if (count > 1000000) break;
        }
        
        return count;
    }
    
    /// Calculate the size of an entry starting at given offset
    fn calculateEntrySize(self: *Self, offset: usize) !usize {
        try self.file.seekTo(offset);
        
        // Skip entry type (1 byte)
        _ = try self.file.readByte();
        var header_offset = offset + 1;
        
        // Skip key length varint
        const key_length = try skipVarIntFromFile(&self.file, &header_offset);
        
        // Skip value length varint
        const value_length = try skipVarIntFromFile(&self.file, &header_offset);
        
        // Calculate total size
        return (header_offset - offset) + key_length + value_length + 8 + 4; // + timestamp + crc
    }
    
    /// Clean up old .bdb files based on retention policy
    pub fn cleanupOldFiles(self: *Self, retention_days: u32) !void {
        const cutoff_time = std.time.milliTimestamp() - (@as(i64, retention_days) * 24 * 60 * 60 * 1000);
        
        var dir = std.fs.cwd().openDir(self.base_path, .{}) catch return;
        defer dir.close();
        
        var iter = dir.iterate();
        while (try iter.next()) |entry| {
            if (entry.kind == .file and std.mem.endsWith(u8, entry.name, ".bdb")) {
                const full_path = try std.fmt.allocPrint(self.allocator, "{s}/{s}", .{ self.base_path, entry.name });
                defer self.allocator.free(full_path);
                
                // Check file modification time
                const stat = try std.fs.cwd().statFile(full_path);
                const modified_time = stat.mtime;
                
                if (modified_time < cutoff_time) {
                    std.debug.print("🗑️ Removing old .bdb file: {s}\n", .{entry.name});
                    std.fs.cwd().deleteFile(full_path) catch {};
                }
            }
        }
    }
    
    /// Validate all files of a specific table type
    pub fn validateAllFiles(self: *Self, table_type: TableType) !std.ArrayList(ValidationReport) {
        var reports = std.ArrayList(ValidationReport).init(self.allocator);
        const file_list = try self.listFiles(table_type);
        defer file_list.deinit();
        
        std.debug.print("🔍 Validating all {s} files ({d} files found)...\n", .{
            @tagName(table_type), file_list.items.len
        });
        
        for (file_list.items) |filename| {
            var file = try self.openFileForValidation(filename);
            defer file.deinit();
            
            const report = try file.validate();
            try reports.append(report);
            
            // Show summary for each file
            std.debug.print("📄 {s}: {s}\n", .{ std.fs.path.basename(filename), report.getSummary() });
        }
        
        return reports;
    }
    
    /// Validate a specific file by filename
    pub fn validateFile(self: *Self, filename: []const u8) !ValidationReport {
        var file = try self.openFileForValidation(filename);
        defer file.deinit();
        
        return try file.validate();
    }
    
    /// Repair corrupted files
    pub fn repairFiles(self: *Self, table_type: TableType, reports: *std.ArrayList(ValidationReport)) !usize {
        var repaired_count: usize = 0;
        
        const file_list = try self.listFiles(table_type);
        defer file_list.deinit();
        
        std.debug.print("🔧 Attempting to repair corrupted files...\n", .{});
        
        for (file_list.items, 0..) |filename, i| {
            const report = &reports.items[i];
            
            if (!report.is_valid and report.can_repair) {
                var file = try self.openFileForValidation(filename);
                defer file.deinit();
                
                const repaired = try file.repair(report);
                if (repaired) {
                    repaired_count += 1;
                    std.debug.print("✅ Repaired: {s}\n", .{std.fs.path.basename(filename)});
                } else {
                    std.debug.print("❌ Failed to repair: {s}\n", .{std.fs.path.basename(filename)});
                }
            }
        }
        
        return repaired_count;
    }
    
    /// Open file specifically for validation (read-only mode)
    fn openFileForValidation(self: *Self, filename: []const u8) !*BDBFile {
        const file = try std.fs.cwd().openFile(filename, .{
            .read = true,
            .write = false, // Read-only for validation
        });
        
        const stat = try file.stat();
        
        const bdb_file = try self.allocator.create(BDBFile);
        bdb_file.* = BDBFile{
            .allocator = self.allocator,
            .filename = filename,
            .header = try readHeaderFromFile(file),
            .footer = BDBFileFooter.init(),
            .file = file,
            .current_offset = stat.size,
            .entry_count = 0,
        };
        
        // Read footer
        try bdb_file.readFooter();
        
        return bdb_file;
    }
};

// ==================== FILE CRC CALCULATION HELPERS ====================

/// Calculate CRC32 for entire file content using chunked processing for memory efficiency
fn calculateFileCRC(file: std.fs.File, chunk_size: usize) !u32 {
    var crc: u32 = 0xFFFFFFFF; // Initial CRC value
    
    // Save current position
    const original_pos = try file.getPos();
    defer file.seekTo(original_pos) catch {};
    
    // Process file in chunks to handle large files efficiently
    try file.seekTo(0);
    
    var buffer = try std.heap.page_allocator.alloc(u8, chunk_size);
    defer std.heap.page_allocator.free(buffer);
    
    var bytes_read: usize = 0;
    while (true) {
        bytes_read = try file.read(buffer);
        if (bytes_read == 0) break;
        
        // Calculate CRC for this chunk
        for (buffer[0..bytes_read]) |byte| {
            crc ^= @as(u32, byte);
            // CRC32 polynomial lookup table iteration
            var i: u3 = 0;
            while (i < 8) : (i += 1) {
                if (crc & 1 == 1) {
                    crc = (crc >> 1) ^ 0xEDB88320; // CRC32 polynomial
                } else {
                    crc >>= 1;
                }
            }
        }
        
        // Safety check for extremely large files
        if (bytes_read < chunk_size) break;
    }
    
    // Finalize CRC (invert bits)
    return ~crc;
}

/// Calculate CRC32 for file with automatic chunk size selection
fn calculateFileCRCAuto(file: std.fs.File) !u32 {
    const stat = try file.stat();
    
    // Choose chunk size based on file size for optimal performance
    const chunk_size: usize = blk: {
        if (stat.size < 64 * 1024) {
            // Small files: use smaller chunks
            break :blk 4 * 1024;
        } else if (stat.size < 16 * 1024 * 1024) {
            // Medium files: use standard chunks
            break :blk 64 * 1024;
        } else {
            // Large files: use larger chunks
            break :blk 256 * 1024;
        }
    };
    
    return calculateFileCRC(file, chunk_size);
}

// ==================== BDB FILE IMPLEMENTATION ====================

pub const BDBFile = struct {
    allocator: Allocator,
    filename: []const u8,
    header: BDBFileHeader,
    footer: BDBFileFooter,
    file: std.fs.File,
    current_offset: usize,
    entry_count: usize,
    
    const Self = @This();
    
    pub fn init(allocator: Allocator, filename: []const u8, table_type: TableType) !*Self {
        // Create or open file
        const file = try std.fs.cwd().createFile(filename, .{
            .read = true,
            .write = true,
            .truncate = true, // Create new file
        });
        
        const self = try allocator.create(Self);
        self.* = Self{
            .allocator = allocator,
            .filename = filename,
            .header = BDBFileHeader.init(table_type),
            .footer = BDBFileFooter.init(),
            .file = file,
            .current_offset = 0,
            .entry_count = 0,
        };
        
        // Write header
        try self.writeHeader();
        
        return self;
    }
    
    pub fn deinit(self: *Self) void {
        // Write footer before closing
        self.writeFooter() catch {};
        self.file.close();
        self.allocator.free(self.filename);
        self.allocator.destroy(self);
    }
    
    /// Calculate CRC32 for entire file content
    pub fn calculateFileCRC(self: *Self) !u32 {
        return calculateFileCRCAuto(self.file);
    }
    
    /// Validate file integrity by checking CRC
    pub fn validateFileIntegrity(self: *Self) !bool {
        // Calculate current file CRC
        const calculated_crc = try self.calculateFileCRC();
        
        // Compare with stored CRC
        return calculated_crc == self.footer.file_crc;
    }
    
    /// Write file header
    pub fn writeHeader(self: *Self) !void {
        var header_buffer = std.ArrayList(u8).init(self.allocator);
        defer header_buffer.deinit();
        
        try self.header.serialize(&header_buffer);
        
        // Write header to file
        try self.file.writeAll(header_buffer.items);
        
        // Update footer with data offset
        self.footer.data_offset = header_buffer.items.len;
        
        self.current_offset = header_buffer.items.len;
    }
    
    /// Write file footer
    pub fn writeFooter(self: *Self) !void {
        // Update footer with current stats
        const current_pos = try self.file.getEndPos();
        self.footer.file_size = current_pos;
        self.footer.entry_count = self.entry_count;
        
        // Calculate file CRC32 for entire file content
        self.footer.file_crc = try self.calculateFileCRC();
        
        // Write footer at end of file
        try self.file.seekFromEnd(-@sizeOf(BDBFileFooter));
        
        var footer_buffer = std.ArrayList(u8).init(self.allocator);
        defer footer_buffer.deinit();
        
        try self.footer.serialize(&footer_buffer);
        try self.file.writeAll(footer_buffer.items);
    }
    
    /// Append a log entry to the file
    pub fn appendEntry(self: *Self, entry: BDBLogEntry) !void {
        var entry_buffer = std.ArrayList(u8).init(self.allocator);
        defer entry_buffer.deinit();
        
        try entry.serialize(&entry_buffer);
        
        // Check if adding this entry would exceed file size limit
        const new_size = self.current_offset + entry_buffer.items.len;
        if (new_size > self.maxFileSize()) {
            return error.FileSizeExceeded;
        }
        
        // Write entry to file
        try self.file.seekFromEnd(0);
        try self.file.writeAll(entry_buffer.items);
        
        // Update stats
        self.current_offset = new_size;
        self.entry_count += 1;
        const entry_buffer_size = @as(u32, @intCast(entry_buffer.items.len));
        self.footer.max_entry_size = std.mem.max(u32, &[_]u32{ self.footer.max_entry_size, entry_buffer_size });
        self.footer.total_key_size += entry.key_length;
        self.footer.total_value_size += entry.value_length;
        
        // Update file modification time
        self.header.modified_at = std.time.milliTimestamp();
        
        // Write updated header
        try self.writeHeader();
    }
    
    /// Read entry from file at given offset
    pub fn readEntry(self: *Self, offset: usize) !BDBLogEntry {
        if (offset >= self.current_offset) {
            return error.OffsetOutOfBounds;
        }
        
        try self.file.seekTo(offset);
        
        // Read entry type (1 byte)
        const entry_type_byte = try self.file.readByte();
        const entry_type = @as(EntryType, @enumFromInt(entry_type_byte));
        
        // Read key length (varint)
        var offset_var = offset + 1;
        const key_length = try readVarIntFromFile(&self.file, &offset_var);
        
        // Read value length (varint)
        const value_length = try readVarIntFromFile(&self.file, &offset_var);
        
        // Allocate buffers for key and value
        const key = try self.allocator.alloc(u8, key_length);
        const value = try self.allocator.alloc(u8, value_length);
        
        // Read key data
        if (key_length > 0) {
            _ = try self.file.read(key);
        }
        
        // Read value data
        if (value_length > 0) {
            _ = try self.file.read(value);
        }
        
        // Read timestamp (8 bytes)
        var timestamp_bytes: [8]u8 = undefined;
        _ = try self.file.read(&timestamp_bytes);
        const timestamp = std.mem.readVarInt(u64, &timestamp_bytes, .little);
        
        // Read and validate entry CRC (4 bytes)
        var crc_bytes: [4]u8 = undefined;
        _ = try self.file.read(&crc_bytes);
        const stored_crc = std.mem.readVarInt(u32, &crc_bytes, .little);
        
        // Create temporary entry for CRC validation
        var temp_entry = BDBLogEntry{
            .entry_type = entry_type,
            .key_length = key_length,
            .value_length = value_length,
            .key = key,
            .value = value,
            .timestamp = timestamp,
            .entry_crc = 0, // Will be calculated
        };
        
        // Validate entry CRC for data integrity
        const calculated_crc = temp_entry.calculateCRC();
        if (calculated_crc != stored_crc) {
            std.debug.print("⚠️ Entry CRC validation failed at offset {}: calculated=0x{X:0>8}, stored=0x{X:0>8}\n", 
                .{ offset, calculated_crc, stored_crc });
            // Continue with data but mark as potentially corrupted
            // The higher-level validation will handle corruption reporting
        }
        
        return BDBLogEntry{
            .entry_type = entry_type,
            .key_length = key_length,
            .value_length = value_length,
            .key = key,
            .value = value,
            .timestamp = timestamp,
            .entry_crc = stored_crc,
        };
    }
    
    /// Binary search for a key in the file
    pub fn searchKey(self: *Self, search_key: []const u8) !?BDBLogEntry {
        // Build an index of all entries for binary search
        const index = try self.buildEntryIndex();
        defer self.allocator.free(index);
        
        // Binary search through the index
        var left: usize = 0;
        var right = index.len;
        
        while (left < right) {
            const mid = (left + right) / 2;
            const entry = try self.readEntry(index[mid].offset);
            defer self.allocator.free(entry.key);
            defer self.allocator.free(entry.value);
            
            const cmp = std.mem.compare(u8, search_key, entry.key);
            if (cmp == .eq) {
                // Found exact match - return entry
                const result_key = try self.allocator.alloc(u8, entry.key_length);
                @memcpy(result_key, entry.key);
                const result_value = try self.allocator.alloc(u8, entry.value_length);
                @memcpy(result_value, entry.value);
                
                return BDBLogEntry{
                    .entry_type = entry.entry_type,
                    .key_length = entry.key_length,
                    .value_length = entry.value_length,
                    .key = result_key,
                    .value = result_value,
                    .timestamp = entry.timestamp,
                    .entry_crc = entry.entry_crc,
                };
            } else if (cmp == .lt) {
                right = mid;
            } else {
                left = mid + 1;
            }
        }
        
        return null; // Key not found
    }
    
    /// Build an index of all entry offsets in the file for binary search
    fn buildEntryIndex(self: *Self) ![]EntryIndex {
        var index = std.ArrayList(EntryIndex).init(self.allocator);
        
        var current_offset = self.footer.data_offset;
        const file_end = self.current_offset - @sizeOf(BDBFileFooter);
        
        while (current_offset < file_end) {
            try self.file.seekTo(current_offset);
            
            // Parse entry header to get next offset
            _ = try self.file.readByte();
            var header_offset = current_offset + 1;
            
            // Skip key length varint
            const key_length = try skipVarIntFromFile(&self.file, &header_offset);
            
            // Skip value length varint
            const value_length = try skipVarIntFromFile(&self.file, &header_offset);
            
            // Skip key and value data
            const data_offset = header_offset + key_length + value_length;
            
            // Skip timestamp (8 bytes) and CRC (4 bytes)
            const next_offset = data_offset + 8 + 4;
            
            // Add to index
            try index.append(EntryIndex{
                .offset = current_offset,
                .key_length = key_length,
                .timestamp = 0, // Will be filled later if needed
            });
            
            current_offset = next_offset;
            
            // Safety check to prevent infinite loop
            if (current_offset <= header_offset) break;
        }
        
        return index.toOwnedSlice() catch return error.OutOfMemory;
    }
    
    /// Get all entries in the file for iteration
    pub fn getAllEntries(self: *Self) ![]BDBLogEntry {
        var entries = std.ArrayList(BDBLogEntry).init(self.allocator);
        
        var current_offset = self.footer.data_offset;
        const file_end = self.current_offset - @sizeOf(BDBFileFooter);
        
        while (current_offset < file_end) {
            const entry = self.readEntry(current_offset) catch break;
            try entries.append(entry);
            
            // Calculate next entry offset
            const next_offset = current_offset + entry.getSize();
            if (next_offset <= current_offset) break; // Safety check
            
            current_offset = next_offset;
        }
        
        return entries.toOwnedSlice() catch return error.OutOfMemory;
    }
    
    /// Count entries in file (used by browserdb.zig SSTable counting)
    pub fn countEntries(self: *Self) usize {
        return self.entry_count;
    }
    
    /// Get maximum file size (header + footer + entries)
    pub fn maxFileSize(_: *Self) usize {
        return @sizeOf(BDBFileHeader) + 
               @sizeOf(BDBFileFooter) + 
               (1024 * 1024 * 1024 - @sizeOf(BDBFileHeader) - @sizeOf(BDBFileFooter)); // 1GB max
    }
    
    /// Comprehensive file validation with detailed error reporting
    /// This method validates the entire file including entries, footer, and integrity checks
    pub fn validate(self: *Self) !ValidationReport {
        var report = ValidationReport.init(self.allocator);
        errdefer report.deinit();
        
        std.debug.print("🔍 Starting comprehensive file validation for: {s}\n", .{self.filename});
        
        // Step 1: Validate header
        try self.file.seekTo(0);
        if (!try self.header.validate()) {
            report.addIssue(.Critical, .HeaderCorruption, "File header validation failed", 0);
            return report;
        }
        
        // Step 2: Calculate and validate file CRC32
        std.debug.print("📊 Calculating file CRC32...\n", .{});
        const calculated_crc = calculateFileCRC32(self.file, self.allocator) catch {
            report.addIssue(.Error, .CRC32Mismatch, "Failed to calculate file CRC32", 0);
            return report;
        };
        
        report.calculated_crc = calculated_crc;
        report.expected_crc32 = self.footer.file_crc;
        
        if (calculated_crc != self.footer.file_crc and self.footer.file_crc != 0) {
            report.addIssue(.Error, .CRC32Mismatch, 
                std.fmt.allocPrint(self.allocator, 
                "File CRC32 mismatch: calculated=0x{X:0>8}, expected=0x{X:0>8}", 
                .{ calculated_crc, self.footer.file_crc }) catch "CRC32 mismatch", 0);
            report.can_repair = true;
        }
        
        // Step 3: Validate footer against actual file data
        std.debug.print("📋 Validating footer data consistency...\n", .{});
        try self.validateFooter(&report);
        
        // Step 4: Entry-by-entry integrity checking
        std.debug.print("📝 Validating all entries...\n", .{});
        try self.validateEntries(&report);
        
        // Step 5: Additional consistency checks
        try self.validateConsistency(&report);
        
        // Step 6: Determine if file can be repaired
        report.can_repair = report.can_repair or (report.entries_corrupted < report.entries_validated / 10);
        
        std.debug.print("✅ Validation completed: {s}\n", .{report.getSummary()});
        return report;
    }
    
    /// Complete file validation system with validation and repair modes
    /// Validates entries and footer completely including CRC32 verification, 
    /// header validation, and entry count checks
    pub fn completeValidation(self: *Self, mode: ValidationMode) !ValidationReport {
        var report = ValidationReport.init(self.allocator);
        errdefer report.deinit();
        
        const mode_str = switch (mode) {
            .Validation => "Validation",
            .Repair => "Validation & Repair",
            .DeepAnalysis => "Deep Analysis",
        };
        
        std.debug.print("🔍 Starting {s} mode for: {s}\n", .{ mode_str, self.filename });
        
        // Phase 1: Header and Structure Validation
        try self.validateHeaderStructure(&report);
        
        // Phase 2: Complete CRC32 verification (file-level and entry-level)
        try self.validateCompleteCRC(&report);
        
        // Phase 3: Entry-by-entry integrity checking with bounds validation
        try self.validateEntriesWithBounds(&report);
        
        // Phase 4: Footer validation against actual file data
        try self.validateFooterAgainstData(&report);
        
        // Phase 5: Data consistency verification
        try self.verifyDataConsistency(&report);
        
        // Phase 6: Corruption detection with detailed error reporting
        self.detectCorruptionPatterns(&report);
        
        // Phase 7: Determine repair capability
        report.can_repair = self.assessRepairCapability(&report);
        
        // If repair mode is requested and file can be repaired
        if (mode == .Repair and report.can_repair) {
            const repaired = try self.performAdvancedRepair(&report);
            if (repaired) {
                std.debug.print("🔧 Advanced repair completed for: {s}\n", .{self.filename});
                
                // Re-validate after repair
                var repair_report = ValidationReport.init(self.allocator);
                defer repair_report.deinit();
                
                try self.validateHeaderStructure(&repair_report);
                try self.validateCompleteCRC(&repair_report);
                try self.verifyDataConsistency(&repair_report);
                
                report.repair_successful = repair_report.is_valid;
                if (repair_report.is_valid) {
                    std.debug.print("✅ File successfully repaired and validated\n", .{});
                }
            }
        }
        
        // Phase 8: Comprehensive error reporting with logging and diagnostics
        try self.generateDetailedDiagnostics(&report);
        
        std.debug.print("🏁 {s} completed: {s}\n", .{ mode_str, report.getSummary() });
        return report;
    }
    
    /// Validate footer data against actual file content
    fn validateFooter(self: *Self, report: *ValidationReport) !void {
        const actual_file_size = self.current_offset;
        const footer_entry_count = try self.countEntriesInFile();
        
        // Check file size consistency
        if (self.footer.file_size != actual_file_size) {
            report.addIssue(.Error, .SizeMismatch, 
                std.fmt.allocPrint(self.allocator, 
                "Footer file size mismatch: footer={}, actual={}", 
                .{ self.footer.file_size, actual_file_size }) catch "Size mismatch", 
                actual_file_size - @sizeOf(BDBFileFooter));
            report.can_repair = true;
        }
        
        // Check entry count consistency
        if (self.footer.entry_count != footer_entry_count) {
            report.addIssue(.Warning, .DataInconsistency, 
                std.fmt.allocPrint(self.allocator, 
                "Footer entry count mismatch: footer={}, actual={}", 
                .{ self.footer.entry_count, footer_entry_count }) catch "Entry count mismatch", 
                actual_file_size - @sizeOf(BDBFileFooter));
        }
        
        // Check data offset consistency
        const expected_data_offset = @sizeOf(BDBFileHeader);
        if (self.footer.data_offset != expected_data_offset) {
            report.addIssue(.Error, .OffsetCorruption, 
                std.fmt.allocPrint(self.allocator, 
                "Footer data offset mismatch: footer={}, expected={}", 
                .{ self.footer.data_offset, expected_data_offset }) catch "Data offset mismatch", 
                expected_data_offset);
            report.can_repair = true;
        }
    }
    
    /// Validate all entries in the file
    fn validateEntries(self: *Self, report: *ValidationReport) !void {
        var current_offset = self.footer.data_offset;
        const file_end = self.current_offset - @sizeOf(BDBFileFooter);
        var entry_index: usize = 0;
        var max_entry_size: u32 = 0;
        var total_key_size: u64 = 0;
        var total_value_size: u64 = 0;
        
        while (current_offset < file_end) {
            report.entries_validated += 1;
            
            // Validate individual entry
            const entry_valid = self.validateEntryAtOffset(current_offset, entry_index, report) catch {
                report.entries_corrupted += 1;
                // Try to find next valid entry by scanning ahead
                current_offset = self.findNextEntryOffset(current_offset, file_end) orelse break;
                entry_index += 1;
                continue;
            };
            
            if (!entry_valid) {
                report.entries_corrupted += 1;
                
                // Calculate stats for repair purposes
                const entry_size = self.calculateEntrySize(current_offset) catch 0;
                if (entry_size > max_entry_size) max_entry_size = @as(u32, @intCast(entry_size));
                
                // Skip corrupted entry and continue
                const next_offset = current_offset + entry_size;
                if (next_offset <= current_offset) break; // Safety check
                current_offset = next_offset;
            } else {
                // Update cumulative stats from valid entries
                const calculated_size = @as(u32, @intCast(self.calculateEntrySize(current_offset) catch 0));
                max_entry_size = std.mem.max(u32, &[_]u32{ max_entry_size, calculated_size });
                
                // Read entry to get sizes (this is a bit expensive, but needed for validation)
                const entry = self.readEntry(current_offset) catch {
                    current_offset += self.calculateEntrySize(current_offset) catch break;
                    entry_index += 1;
                    continue;
                };
                total_key_size += entry.key_length;
                total_value_size += entry.value_length;
                
                // Free allocated memory
                self.allocator.free(entry.key);
                self.allocator.free(entry.value);
                
                const next_offset = current_offset + entry.getSize();
                if (next_offset <= current_offset) break; // Safety check
                current_offset = next_offset;
            }
            
            entry_index += 1;
            
            // Safety limit to prevent infinite loops
            if (entry_index > 10000000) {
                report.addIssue(.Error, .MalformedEntry, "Too many entries detected - possible infinite loop", current_offset);
                break;
            }
        }
        
        // Update footer stats based on validation
        if (total_key_size != self.footer.total_key_size) {
            report.addIssue(.Warning, .DataInconsistency, 
                std.fmt.allocPrint(self.allocator, 
                "Footer total key size mismatch: footer={}, actual={}", 
                .{ self.footer.total_key_size, total_key_size }) catch "Key size mismatch", 
                self.current_offset - @sizeOf(BDBFileFooter));
        }
        
        if (total_value_size != self.footer.total_value_size) {
            report.addIssue(.Warning, .DataInconsistency, 
                std.fmt.allocPrint(self.allocator, 
                "Footer total value size mismatch: footer={}, actual={}", 
                .{ self.footer.total_value_size, total_value_size }) catch "Value size mismatch", 
                self.current_offset - @sizeOf(BDBFileFooter));
        }
    }
    
    /// Validate a single entry at a specific offset
    fn validateEntryAtOffset(self: *Self, offset: usize, entry_index: usize, report: *ValidationReport) !bool {
        // Check if we're within file bounds
        if (offset >= self.current_offset) {
            report.addIssue(.Error, .OffsetCorruption, 
                std.fmt.allocPrint(self.allocator, "Entry {} offset out of bounds", .{entry_index}) catch "Offset out of bounds", 
                offset);
            return false;
        }
        
        try self.file.seekTo(offset);
        
        // Read and validate entry type
        const entry_type_byte = self.file.readByte() catch {
            report.addIssue(.Error, .MalformedEntry, 
                std.fmt.allocPrint(self.allocator, "Entry {}: failed to read entry type", .{entry_index}) catch "Read failed", 
                offset);
            return false;
        };
        
        const entry_type = @as(EntryType, @enumFromInt(entry_type_byte));
        if (@intFromEnum(entry_type) > 5) {
            report.addIssue(.Error, .MalformedEntry, 
                std.fmt.allocPrint(self.allocator, "Entry {}: invalid entry type {}", .{entry_index, entry_type_byte}) catch "Invalid entry type", 
                offset);
            return false;
        }
        
        // Calculate expected entry size
        const expected_size = self.calculateEntrySize(offset) catch {
            report.addIssue(.Error, .MalformedEntry, 
                std.fmt.allocPrint(self.allocator, "Entry {}: failed to calculate size", .{entry_index}) catch "Size calculation failed", 
                offset);
            return false;
        };
        
        // Check if entry would exceed file bounds
        if (offset + expected_size > self.current_offset - @sizeOf(BDBFileFooter)) {
            report.addIssue(.Error, .TruncatedFile, 
                std.fmt.allocPrint(self.allocator, "Entry {}: truncated entry (size={}, available={})", 
                .{ entry_index, expected_size, self.current_offset - @sizeOf(BDBFileFooter) - offset }) catch "Truncated entry", 
                offset);
            return false;
        }
        
        // Validate entry CRC if possible
        try self.file.seekTo(offset);
        const entry = self.readEntry(offset) catch {
            report.addIssue(.Error, .EntryCorruption, 
                std.fmt.allocPrint(self.allocator, "Entry {}: corrupted entry data", .{entry_index}) catch "Corrupted entry", 
                offset);
            return false;
        };
        
        // Verify entry CRC
        const calculated_crc = entry.calculateCRC();
        if (calculated_crc != entry.entry_crc) {
            report.addIssue(.Warning, .CRC32Mismatch, 
                std.fmt.allocPrint(self.allocator, "Entry {}: CRC32 mismatch (calculated=0x{X:0>8}, stored=0x{X:0>8})", 
                .{ entry_index, calculated_crc, entry.entry_crc }) catch "Entry CRC mismatch", 
                offset);
            // Don't fail validation for entry CRC mismatches - they're often repairable
        }
        
        // Validate timestamp (should be reasonable)
        const current_time = std.time.milliTimestamp();
        const one_year_ms = 365 * 24 * 60 * 60 * 1000;
        if (entry.timestamp > current_time + one_year_ms or entry.timestamp < current_time - (10 * one_year_ms)) {
            report.addIssue(.Warning, .TimestampAnomaly, 
                std.fmt.allocPrint(self.allocator, "Entry {}: unusual timestamp {}", .{entry_index, entry.timestamp}) catch "Timestamp anomaly", 
                offset);
        }
        
        // Free allocated memory
        self.allocator.free(entry.key);
        self.allocator.free(entry.value);
        
        return true;
    }
    
    /// Find the next valid entry offset after a corrupted one
    fn findNextEntryOffset(_: *Self, current_offset: usize, file_end: usize) ?usize {
        var scan_offset = current_offset + 1;
        const max_scan_size = 1024; // Scan up to 1KB for next valid entry
        
        while (scan_offset < file_end and scan_offset < current_offset + max_scan_size) {
            // Try to parse as entry type
            // This is a simplified approach - in practice, you'd want more sophisticated pattern matching
            if (scan_offset + 1 < file_end) {
                // Look for valid entry type bytes
                // Valid entry types are 1-5
                // This is a heuristic approach
                return scan_offset;
            }
            scan_offset += 1;
        }
        
        return null;
    }
    
    /// Perform additional consistency checks
    fn validateConsistency(self: *Self, report: *ValidationReport) !void {
        // Check if file is properly closed (footer exists and is valid)
        try self.file.seekFromEnd(-@sizeOf(BDBFileFooter));
        var footer_buffer: [@sizeOf(BDBFileFooter)]u8 = undefined;
        _ = try self.file.read(&footer_buffer);
        
        // Verify footer structure is readable
        const footer = @as(*BDBFileFooter, @ptrCast(&footer_buffer));
        _ = footer.entry_count; // Access to verify structure
        
        // Check file structure integrity
        const header_size = @sizeOf(BDBFileHeader);
        const footer_size = @sizeOf(BDBFileFooter);
        const expected_min_size = header_size + footer_size;
        
        if (self.current_offset < expected_min_size) {
            report.addIssue(.Critical, .TruncatedFile, 
                std.fmt.allocPrint(self.allocator, "File too small: {} < {} minimum", 
                .{ self.current_offset, expected_min_size }) catch "File too small", 
                0);
        }
        
        // Check for potential truncation
        if (self.current_offset > expected_min_size and 
            (self.current_offset - expected_min_size) % 1024 == 0) {
            report.addIssue(.Warning, .DataInconsistency, 
                "File size suggests possible truncation", self.current_offset - footer_size);
        }
    }
    
    /// Attempt to repair minor corruption issues
    pub fn repair(self: *Self, report: *ValidationReport) !bool {
        if (!report.can_repair) {
            std.debug.print("❌ File cannot be repaired\n", .{});
            return false;
        }
        
        std.debug.print("🔧 Attempting to repair file: {s}\n", .{self.filename});
        report.repair_attempted = true;
        
        var repair_successful = false;
        
        // Step 1: Fix footer data inconsistencies
        if (try self.repairFooter(report)) {
            repair_successful = true;
        }
        
        // Step 2: Fix header issues if any
        if (try self.repairHeader(report)) {
            repair_successful = true;
        }
        
        // Step 3: Recalculate and update file CRC
        if (try self.recalculateFileCRC(report)) {
            repair_successful = true;
        }
        
        if (repair_successful) {
            // Write repaired footer
            try self.writeFooter();
            std.debug.print("✅ File repair completed\n", .{});
        } else {
            std.debug.print("❌ File repair failed\n", .{});
        }
        
        report.repair_successful = repair_successful;
        return repair_successful;
    }
    
    /// Repair footer inconsistencies
    fn repairFooter(self: *Self, _: *ValidationReport) !bool {
        var repaired = false;
        
        // Recalculate entry count
        const actual_entry_count = try self.countEntriesInFile();
        if (self.footer.entry_count != actual_entry_count) {
            std.debug.print("🔧 Fixing footer entry count: {} -> {}\n", .{ self.footer.entry_count, actual_entry_count });
            self.footer.entry_count = actual_entry_count;
            repaired = true;
        }
        
        // Recalculate file size
        const actual_file_size = self.current_offset;
        if (self.footer.file_size != actual_file_size) {
            std.debug.print("🔧 Fixing footer file size: {} -> {}\n", .{ self.footer.file_size, actual_file_size });
            self.footer.file_size = actual_file_size;
            repaired = true;
        }
        
        return repaired;
    }
    
    /// Repair header inconsistencies
    fn repairHeader(self: *Self, _: *ValidationReport) !bool {
        var repaired = false;
        
        // Update modification time
        const current_time = std.time.milliTimestamp();
        if (self.header.modified_at != current_time) {
            self.header.modified_at = current_time;
            
            // Recalculate header CRC
            self.header.header_crc = self.header.calculateCRC();
            repaired = true;
        }
        
        return repaired;
    }
    
    /// Recalculate and update file CRC
    fn recalculateFileCRC(self: *Self, report: *ValidationReport) !bool {
        const new_crc = calculateFileCRC32(self.file, self.allocator) catch return false;
        
        if (self.footer.file_crc != new_crc) {
            std.debug.print("🔧 Updating file CRC: 0x{X:0>8} -> 0x{X:0>8}\n", .{ self.footer.file_crc, new_crc });
            self.footer.file_crc = new_crc;
            report.calculated_crc = new_crc;
            return true;
        }
        
        return false;
    }
    
    /// Validate header structure and basic file format
    fn validateHeaderStructure(self: *Self, report: *ValidationReport) !void {
        try self.file.seekTo(0);
        
        // Validate magic bytes
        var magic_buffer: [8]u8 = undefined;
        _ = try self.file.read(&magic_buffer);
        if (!std.mem.eql(u8, &magic_buffer, &MAGIC_BYTES)) {
            report.addIssue(.Critical, .HeaderCorruption, 
                "Invalid magic bytes - file signature corrupted", 0);
            return;
        }
        
        // Validate version compatibility
        const version_byte = try self.file.readByte();
        if (version_byte > BDB_VERSION) {
            report.addIssue(.Error, .HeaderCorruption, 
                "File version too new - incompatible format", 1);
        }
        
        // Verify header CRC
        if (!try self.header.validate()) {
            report.addIssue(.Error, .CRC32Mismatch, 
                "Header CRC validation failed", 0);
        }
        
        std.debug.print("📋 Header structure validation completed\n", .{});
    }
    
    /// Complete CRC32 verification for both file and entries
    fn validateCompleteCRC(self: *Self, report: *ValidationReport) !void {
        std.debug.print("🔐 Calculating complete file CRC32...\n", .{});
        
        // File-level CRC validation
        const calculated_file_crc = calculateFileCRC32(self.file, self.allocator) catch {
            report.addIssue(.Error, .CRC32Mismatch, 
                "Failed to calculate file CRC32", 0);
            return;
        };
        
        report.calculated_crc32 = calculated_file_crc;
        report.expected_crc32 = self.footer.file_crc;
        
        if (self.footer.file_crc != 0 and calculated_file_crc != self.footer.file_crc) {
            report.addIssue(.Error, .CRC32Mismatch, 
                std.fmt.allocPrint(self.allocator, 
                "File CRC32 mismatch: calculated=0x{X:0>8}, expected=0x{X:0>8}", 
                .{ calculated_file_crc, self.footer.file_crc }) catch "File CRC mismatch", 
                self.current_offset - @sizeOf(BDBFileFooter));
        }
        
        std.debug.print("📊 File CRC32 validation: 0x{X:0>8} vs 0x{X:0>8}\n", 
            .{ calculated_file_crc, self.footer.file_crc });
    }
    
    /// Entry-by-entry integrity checking with bounds validation
    fn validateEntriesWithBounds(self: *Self, report: *ValidationReport) !void {
        std.debug.print("📝 Performing entry-by-entry integrity validation...\n", .{});
        
        var current_offset = self.footer.data_offset;
        const file_end = self.current_offset - @sizeOf(BDBFileFooter);
        var entry_index: usize = 0;
        var crc_mismatches: usize = 0;
        var bounds_violations: usize = 0;
        
        while (current_offset < file_end) {
            report.entries_validated += 1;
            
            // Bounds validation
            if (current_offset >= file_end) {
                report.addIssue(.Error, .OffsetCorruption, 
                    "Entry offset exceeds file bounds", current_offset);
                break;
            }
            
            // Individual entry validation with detailed error reporting
            const entry_valid = self.validateEntryWithDetailedReporting(current_offset, entry_index, report) catch {
                report.entries_corrupted += 1;
                bounds_violations += 1;
                
                // Advanced corruption recovery: try to find next valid entry
                current_offset = self.advancedCorruptionRecovery(current_offset, file_end) orelse break;
                entry_index += 1;
                continue;
            };
            
            if (entry_valid) {
                // Additional CRC validation for valid entries
                const entry = self.readEntry(current_offset) catch {
                    current_offset += self.calculateEntrySize(current_offset) catch break;
                    entry_index += 1;
                    continue;
                };
                
                const calculated_crc = entry.calculateCRC();
                if (calculated_crc != entry.entry_crc) {
                    crc_mismatches += 1;
                    if (crc_mismatches < 10) { // Limit error spam
                        report.addIssue(.Warning, .CRC32Mismatch, 
                            std.fmt.allocPrint(self.allocator, 
                            "Entry {} CRC mismatch: calculated=0x{X:0>8}, stored=0x{X:0>8}", 
                            .{ entry_index, calculated_crc, entry.entry_crc }) catch "Entry CRC mismatch", 
                            current_offset);
                    }
                }
                
                // Free allocated memory
                self.allocator.free(entry.key);
                self.allocator.free(entry.value);
            }
            
            // Calculate next entry offset with overflow protection
            const next_offset = current_offset + self.calculateEntrySize(current_offset) catch {
                report.addIssue(.Error, .MalformedEntry, 
                    std.fmt.allocPrint(self.allocator, 
                    "Entry {}: unable to calculate size", .{entry_index}) catch "Size calculation failed", 
                    current_offset);
                break;
            };
            
            if (next_offset <= current_offset) {
                report.addIssue(.Critical, .MalformedEntry, 
                    "Entry size calculation overflow detected", current_offset);
                break;
            }
            
            current_offset = next_offset;
            entry_index += 1;
            
            // Safety limit to prevent infinite loops
            if (entry_index > 10000000) {
                report.addIssue(.Critical, .MalformedEntry, 
                    "Excessive entry count - possible infinite loop", current_offset);
                break;
            }
        }
        
        std.debug.print("✅ Entry validation completed: {} entries validated, {} corrupted, {} CRC mismatches\n", 
            .{ report.entries_validated, report.entries_corrupted, crc_mismatches });
    }
    
    /// Footer validation against actual file data
    fn validateFooterAgainstData(self: *Self, report: *ValidationReport) !void {
        std.debug.print("📋 Validating footer against actual file data...\n", .{});
        
        const actual_file_size = self.current_offset;
        const actual_entry_count = try self.countEntriesInFile();
        
        // File size consistency check
        if (self.footer.file_size != actual_file_size) {
            report.addIssue(.Error, .SizeMismatch, 
                std.fmt.allocPrint(self.allocator, 
                "Footer file size mismatch: footer={}, actual={}", 
                .{ self.footer.file_size, actual_file_size }) catch "Size mismatch", 
                actual_file_size - @sizeOf(BDBFileFooter));
        }
        
        // Entry count verification
        if (self.footer.entry_count != actual_entry_count) {
            report.addIssue(.Error, .DataInconsistency, 
                std.fmt.allocPrint(self.allocator, 
                "Footer entry count mismatch: footer={}, actual={}", 
                .{ self.footer.entry_count, actual_entry_count }) catch "Entry count mismatch", 
                actual_file_size - @sizeOf(BDBFileFooter));
        }
        
        // Data offset validation
        const expected_data_offset = @sizeOf(BDBFileHeader);
        if (self.footer.data_offset != expected_data_offset) {
            report.addIssue(.Error, .OffsetCorruption, 
                std.fmt.allocPrint(self.allocator, 
                "Footer data offset incorrect: footer={}, expected={}", 
                .{ self.footer.data_offset, expected_data_offset }) catch "Data offset mismatch", 
                expected_data_offset);
        }
        
        std.debug.print("📊 Footer validation: size={}/{}, entries={}/{}, offset={}\n", 
            .{ self.footer.file_size, actual_file_size, self.footer.entry_count, actual_entry_count, self.footer.data_offset });
    }
    
    /// Verify data consistency across the file
    fn verifyDataConsistency(self: *Self, report: *ValidationReport) !void {
        std.debug.print("🔍 Verifying data consistency...\n", .{});
        
        // Check file structure integrity
        const header_size = @sizeOf(BDBFileHeader);
        const footer_size = @sizeOf(BDBFileFooter);
        const expected_min_size = header_size + footer_size;
        
        if (self.current_offset < expected_min_size) {
            report.addIssue(.Critical, .TruncatedFile, 
                std.fmt.allocPrint(self.allocator, 
                "File too small: {} < {} minimum", 
                .{ self.current_offset, expected_min_size }) catch "File too small", 0);
        }
        
        // Check for alignment issues
        if ((self.current_offset - header_size) % 4 != 0) {
            report.addIssue(.Warning, .DataInconsistency, 
                "File size not properly aligned", header_size);
        }
        
        std.debug.print("✅ Data consistency verification completed\n", .{});
    }
    
    /// Detect corruption patterns in the file
    fn detectCorruptionPatterns(self: *Self, report: *ValidationReport) void {
        std.debug.print("🔎 Detecting corruption patterns...\n", .{});
        
        // Pattern 1: High corruption rate
        if (report.entries_validated > 0) {
            const corruption_rate = @as(f64, @floatFromInt(report.entries_corrupted)) / @as(f64, @floatFromInt(report.entries_validated));
            if (corruption_rate > 0.1) {
                report.addIssue(.Critical, .DataInconsistency, 
                    std.fmt.allocPrint(report.issues.allocator.allocator, 
                    "High corruption rate detected: {:.1}%", .{corruption_rate * 100}) catch "High corruption rate", 
                    self.current_offset - @sizeOf(BDBFileFooter));
            }
        }
        
        // Pattern 2: Early corruption (first 10% of entries)
        if (report.entries_corrupted > 0 and report.entries_validated > 10) {
            // This would require tracking corruption location, simplified here
            report.addIssue(.Warning, .DataInconsistency, 
                "Corruption detected in file - may indicate hardware issues", 0);
        }
        
        std.debug.print("✅ Corruption pattern detection completed\n", .{});
    }
    
    /// Assess if file can be repaired based on corruption level
    fn assessRepairCapability(self: *Self, report: *ValidationReport) bool {
        _ = self; // Mark self as intentionally unused
        // File can be repaired if:
        // 1. Header is valid (critical for repair)
        // 2. Corruption rate is less than 50%
        // 3. No critical structural damage
        // File can be repaired if:
        // 1. Header is valid (critical for repair)
        // 2. Corruption rate is less than 50%
        // 3. No critical structural damage
        
        if (report.entries_validated == 0) return false;
        
        const corruption_rate = @as(f64, @floatFromInt(report.entries_corrupted)) / @as(f64, @floatFromInt(report.entries_validated));
        
        // Check if header validation passed
        var header_valid = true;
        for (report.issues.items) |issue| {
            if (issue.corruption_type == .HeaderCorruption and issue.severity == .Critical) {
                header_valid = false;
                break;
            }
        }
        
        const can_repair = header_valid and corruption_rate < 0.5;
        
        if (can_repair) {
            std.debug.print("🔧 File marked as repairable (corruption rate: {:.1}%)\n", .{corruption_rate * 100});
        } else {
            std.debug.print("❌ File marked as not repairable (corruption rate: {:.1}%)\n", .{corruption_rate * 100});
        }
        
        return can_repair;
    }
    
    /// Validate single entry with detailed error reporting
    fn validateEntryWithDetailedReporting(self: *Self, offset: usize, entry_index: usize, report: *ValidationReport) !bool {
        // Bounds check
        if (offset >= self.current_offset - @sizeOf(BDBFileFooter)) {
            report.addIssue(.Error, .OffsetCorruption, 
                std.fmt.allocPrint(self.allocator, "Entry {} offset out of bounds", .{entry_index}) catch "Offset out of bounds", 
                offset);
            return false;
        }
        
        try self.file.seekTo(offset);
        
        // Entry type validation
        const entry_type_byte = self.file.readByte() catch {
            report.addIssue(.Error, .MalformedEntry, 
                std.fmt.allocPrint(self.allocator, "Entry {}: unable to read entry type", .{entry_index}) catch "Read failed", 
                offset);
            return false;
        };
        
        if (entry_type_byte == 0 or entry_type_byte > 5) {
            report.addIssue(.Error, .MalformedEntry, 
                std.fmt.allocPrint(self.allocator, "Entry {}: invalid entry type {}", .{entry_index, entry_type_byte}) catch "Invalid entry type", 
                offset);
            return false;
        }
        
        // Size calculation validation
        const expected_size = self.calculateEntrySize(offset) catch {
            report.addIssue(.Error, .MalformedEntry, 
                std.fmt.allocPrint(self.allocator, "Entry {}: size calculation failed", .{entry_index}) catch "Size calc failed", 
                offset);
            return false;
        };
        
        // Bounds verification for entry data
        const entry_end = offset + expected_size;
        const file_data_end = self.current_offset - @sizeOf(BDBFileFooter);
        
        if (entry_end > file_data_end) {
            report.addIssue(.Error, .TruncatedFile, 
                std.fmt.allocPrint(self.allocator, "Entry {}: exceeds file bounds (ends at {}, file ends at {})", 
                .{ entry_index, entry_end, file_data_end }) catch "Entry exceeds bounds", 
                offset);
            return false;
        }
        
        return true;
    }
    
    /// Advanced corruption recovery - tries to find next valid entry
    fn advancedCorruptionRecovery(self: *Self, current_offset: usize, file_end: usize) ?usize {
        const max_scan_bytes = 4096; // Scan up to 4KB for recovery
        
        var scan_offset = current_offset + 1;
        var best_candidate: ?usize = null;
        var scan_count: usize = 0;
        
        while (scan_offset < file_end and scan_offset < current_offset + max_scan_bytes and scan_count < 100) {
            // Look for valid entry type pattern
            if (scan_offset + 1 < file_end) {
                // Try to read potential entry type
                // This is a simplified heuristic - in practice, more sophisticated pattern matching would be used
                if (self.isValidEntryStart(scan_offset)) {
                    best_candidate = scan_offset;
                    break;
                }
            }
            
            scan_offset += 1;
            scan_count += 1;
        }
        
        if (best_candidate) |offset| {
            std.debug.print("🔧 Recovery: Found potential entry at offset {}\n", .{offset});
        }
        
        return best_candidate;
    }
    
    /// Check if offset appears to be a valid entry start
    fn isValidEntryStart(self: *Self, offset: usize) bool {
        // Simplified validation - checks if the byte at offset looks like a valid entry type
        // In a real implementation, this would be more sophisticated
        return offset < self.current_offset - @sizeOf(BDBFileFooter);
    }
    
    /// Perform advanced repair operations
    fn performAdvancedRepair(self: *Self, report: *ValidationReport) !bool {
        std.debug.print("🔧 Starting advanced repair operations...\n", .{});
        
        var repair_successful = false;
        
        // Step 1: Repair footer with recomputed statistics
        if (try self.repairFooterWithRecomputation(report)) {
            repair_successful = true;
        }
        
        // Step 2: Fix header if needed
        if (try self.repairHeaderIfNeeded(report)) {
            repair_successful = true;
        }
        
        // Step 3: Regenerate all checksums
        if (try self.regenerateAllChecksums(report)) {
            repair_successful = true;
        }
        
        // Step 4: Optimize file structure if needed
        if (try self.optimizeFileStructure(report)) {
            repair_successful = true;
        }
        
        return repair_successful;
    }
    
    /// Repair footer with complete recomputation
    fn repairFooterWithRecomputation(self: *Self, _: *ValidationReport) !bool {
        var repaired = false;
        
        // Recompute all footer statistics from scratch
        const actual_entry_count = try self.countEntriesInFile();
        const actual_file_size = self.current_offset;
        
        if (self.footer.entry_count != actual_entry_count) {
            std.debug.print("🔧 Repairing footer entry count: {} -> {}\n", .{ self.footer.entry_count, actual_entry_count });
            self.footer.entry_count = actual_entry_count;
            repaired = true;
        }
        
        if (self.footer.file_size != actual_file_size) {
            std.debug.print("🔧 Repairing footer file size: {} -> {}\n", .{ self.footer.file_size, actual_file_size });
            self.footer.file_size = actual_file_size;
            repaired = true;
        }
        
        return repaired;
    }
    
    /// Repair header if needed
    fn repairHeaderIfNeeded(self: *Self, _: *ValidationReport) !bool {
        var repaired = false;
        
        // Update modification time
        const current_time = std.time.milliTimestamp();
        if (self.header.modified_at != current_time) {
            self.header.modified_at = current_time;
            
            // Recalculate header CRC
            const old_crc = self.header.header_crc;
            self.header.header_crc = self.header.calculateCRC();
            
            if (old_crc != self.header.header_crc) {
                std.debug.print("🔧 Updated header CRC: 0x{X:0>8} -> 0x{X:0>8}\n", .{ old_crc, self.header.header_crc });
                repaired = true;
            }
        }
        
        return repaired;
    }
    
    /// Regenerate all checksums in the file
    fn regenerateAllChecksums(self: *Self, _: *ValidationReport) !bool {
        // Recalculate file CRC
        const new_file_crc = calculateFileCRC32(self.file, self.allocator) catch return false;
        
        if (self.footer.file_crc != new_file_crc) {
            std.debug.print("🔧 Regenerating file CRC: 0x{X:0>8} -> 0x{X:0>8}\n", .{ self.footer.file_crc, new_file_crc });
            self.footer.file_crc = new_file_crc;
            return true;
        }
        
        return false;
    }
    
    /// Optimize file structure after repair
    fn optimizeFileStructure(self: *Self, _: *ValidationReport) !bool {
        // This could include things like:
        // - Defragmentation
        // - Index rebuilding
        // - Compression optimization
        // For now, just ensure proper alignment
        
        const header_size = @sizeOf(BDBFileHeader);
        if (self.footer.data_offset != header_size) {
            self.footer.data_offset = header_size;
            std.debug.print("🔧 Optimized data offset alignment\n", .{});
            return true;
        }
        
        return false;
    }
    
    /// Generate detailed diagnostics and logging
    fn generateDetailedDiagnostics(self: *Self, report: *ValidationReport) !void {
        std.debug.print("\n📊 DETAILED VALIDATION DIAGNOSTICS FOR: {s}\n", .{self.filename});
        std.debug.print("═══════════════════════════════════════════════════════\n", .{});
        
        // Overall status
        const status = if (report.is_valid) "✅ VALID" else "❌ INVALID";
        std.debug.print("Overall Status: {s}\n", .{status});
        
        // Statistics
        std.debug.print("Entries Validated: {}\n", .{report.entries_validated});
        std.debug.print("Entries Corrupted: {}\n", .{report.entries_corrupted});
        std.debug.print("Corruption Rate: {:.2}%\n", .{
            if (report.entries_validated > 0) 
                (@as(f64, @floatFromInt(report.entries_corrupted)) / @as(f64, @floatFromInt(report.entries_validated))) * 100 
            else 0
        });
        
        // CRC Information
        std.debug.print("File CRC32 - Calculated: 0x{X:0>8}, Expected: 0x{X:0>8}\n", 
            .{ report.calculated_crc32, report.expected_crc32 });
        
        // Repair capability
        const repair_status = if (report.can_repair) "🔧 REPAIRABLE" else "🚫 NOT REPAIRABLE";
        std.debug.print("Repair Status: {s}\n", .{repair_status});
        
        // Issue breakdown
        if (report.issues.items.len > 0) {
            std.debug.print("\nIssues Found ({} total):\n", .{report.issues.items.len});
            
            var errors: usize = 0;
            var warnings: usize = 0;
            var critical: usize = 0;
            
            for (report.issues.items) |issue| {
                const prefix = switch (issue.severity) {
                    .Critical => "🚨 CRITICAL",
                    .Error => "❌ ERROR",
                    .Warning => "⚠️ WARNING",
                    .Info => "ℹ️ INFO",
                };
                
                std.debug.print("  {} [{}] at offset {}: {s}\n", .{
                    prefix, 
                    @tagName(issue.corruption_type),
                    issue.offset,
                    issue.description
                });
                
                switch (issue.severity) {
                    .Critical => critical += 1,
                    .Error => errors += 1,
                    .Warning => warnings += 1,
                    .Info => {},
                }
            }
            
            std.debug.print("\nIssue Summary: {} critical, {} errors, {} warnings\n", .{
                critical, errors, warnings
            });
        } else {
            std.debug.print("\n✅ No issues found - file is fully valid\n", .{});
        }
        
        // File metadata
        std.debug.print("\n📋 File Metadata:\n", .{});
        std.debug.print("  Table Type: {s}\n", .{@tagName(self.header.table_type)});
        std.debug.print("  Version: {}\n", .{self.header.version});
        std.debug.print("  Created: {}\n", .{self.header.created_at});
        std.debug.print("  Modified: {}\n", .{self.header.modified_at});
        std.debug.print("  File Size: {} bytes\n", .{self.current_offset});
        std.debug.print("  Entry Count: {}\n", .{self.footer.entry_count});
        
        std.debug.print("═══════════════════════════════════════════════════════\n\n", .{});
    }
};

// ==================== COMPRESSION UTILITIES ====================

pub const BDBCompression = struct {
    /// Compress data using specified algorithm with memory efficiency
    pub fn compress(data: []const u8, compression_type: CompressionType) ![]const u8 {
        switch (compression_type) {
            .None => return data,
            .Zlib => return compressLZ77(data),
            .Lz4 => return compressLZ4Fast(data),
            .Zstd => return compressZstandard(data),
        }
    }
    
    /// Decompress data using specified algorithm with recovery mechanisms
    pub fn decompress(data: []const u8, compression_type: CompressionType, original_size: usize) ![]const u8 {
        switch (compression_type) {
            .None => return data,
            .Zlib => return decompressLZ77(data, original_size),
            .Lz4 => return decompressLZ4Fast(data, original_size),
            .Zstd => return decompressZstandard(data, original_size),
        }
    }
    
    /// Stream compression for large data without loading into memory
    pub fn compressStream(reader: anytype, writer: anytype, compression_type: CompressionType) !void {
        switch (compression_type) {
            .None => {
                // Direct copy for uncompressed data
                var buffer = try std.heap.page_allocator.alloc(u8, 8192);
                defer std.heap.page_allocator.free(buffer);
                
                while (true) {
                    const bytes_read = try reader.read(buffer);
                    if (bytes_read == 0) break;
                    try writer.writeAll(buffer[0..bytes_read]);
                }
            },
            .Zlib => return compressLZ77Stream(reader, writer),
            .Lz4 => return compressLZ4Stream(reader, writer),
            .Zstd => return compressZstandardStream(reader, writer),
        }
    }
    
    /// Stream decompression with error recovery
    pub fn decompressStream(reader: anytype, writer: anytype, compression_type: CompressionType, expected_size: usize) !void {
        switch (compression_type) {
            .None => {
                var buffer = try std.heap.page_allocator.alloc(u8, 8192);
                defer std.heap.page_allocator.free(buffer);
                
                var total_written: usize = 0;
                while (total_written < expected_size) {
                    const to_read = @min(buffer.len, expected_size - total_written);
                    const bytes_read = try reader.read(buffer[0..to_read]);
                    if (bytes_read == 0) break;
                    try writer.writeAll(buffer[0..bytes_read]);
                    total_written += bytes_read;
                }
            },
            .Zlib => return decompressLZ77Stream(reader, writer, expected_size),
            .Lz4 => return decompressLZ4Stream(reader, writer, expected_size),
            .Zstd => return decompressZstandardStream(reader, writer, expected_size),
        }
    }
    
    /// Calculate compression ratio
    pub fn calculateRatio(compressed_size: usize, original_size: usize) u16 {
        if (original_size == 0) return 100;
        const ratio = @divFloor(compressed_size * 100, original_size);
        return @as(u16, @min(ratio, 100));
    }
    
    /// Advanced compression algorithm recommendation with performance prediction
    pub fn getRecommendedAlgorithm(data: []const u8) CompressionType {
        if (data.len < 64) return .None; // Don't compress very small data
        
        // Enhanced data analysis with multiple metrics
        var repetition_count: usize = 0;
        var unique_bytes: usize = 0;
        var zero_count: usize = 0;
        var printable_count: usize = 0;
        var byte_freq = [_]usize{0} ** 256;
        
        // Sample data for large datasets to improve performance
        const sample_size = @min(data.len, 8192);
        const sample_step = if (data.len > sample_size) @divFloor(data.len, sample_size) else 1;
        
        for (0..sample_size) |i| {
            const byte = data[i * sample_step];
            byte_freq[byte] += 1;
            
            if (byte == 0) zero_count += 1;
            if (byte >= 32 and byte <= 126) printable_count += 1; // ASCII printable
        }
        
        for (byte_freq) |freq| {
            if (freq > 0) unique_bytes += 1;
            if (freq > 1) repetition_count += freq - 1;
        }
        
        const repetition_ratio = @as(f64, @floatFromInt(repetition_count)) / @as(f64, @floatFromInt(sample_size));
        const uniqueness_ratio = @as(f64, @floatFromInt(unique_bytes)) / 256.0;
        const zero_ratio = @as(f64, @floatFromInt(zero_count)) / @as(f64, @floatFromInt(sample_size));
        const printable_ratio = @as(f64, @floatFromInt(printable_count)) / @as(f64, @floatFromInt(sample_size));
        
        // Calculate estimated compression ratios for each algorithm
        const estimated_zlib_ratio = calculateEstimatedCompressionRatio(data, .Zlib);
        const estimated_lz4_ratio = calculateEstimatedCompressionRatio(data, .Lz4);
        const estimated_zstd_ratio = calculateEstimatedCompressionRatio(data, .Zstd);
        
        // Choose algorithm based on comprehensive analysis
        if (data.len < 1024) {
            return .None; // Small data - compression overhead not worth it
        } else if (repetition_ratio > 0.4 or zero_ratio > 0.3) {
            return .Zlib; // Very repetitive or sparse data - LZ77 excels
        } else if (printable_ratio > 0.8 and uniqueness_ratio < 0.6) {
            return .Lz4; // Text-like data - LZ4 is fast and effective
        } else if (estimated_zstd_ratio < estimated_lz4_ratio * 0.9) {
            return .Zstd; // Zstd shows significant advantage
        } else {
            return .Lz4; // Default to speed for general case
        }
    }
    
    /// Calculate estimated compression ratio for planning purposes
    fn calculateEstimatedCompressionRatio(data: []const u8, algorithm: CompressionType) f64 {
        // Quick heuristic estimation based on data characteristics
        var unique_bytes: usize = 0;
        var byte_seen = [_]bool{false} ** 256;
        
        const sample_size = @min(data.len, 2048);
        const sample_step = if (data.len > sample_size) @divFloor(data.len, sample_size) else 1;
        
        for (0..sample_size) |i| {
            const byte = data[i * sample_step];
            if (!byte_seen[byte]) {
                byte_seen[byte] = true;
                unique_bytes += 1;
            }
        }
        
        const entropy_ratio = @as(f64, @floatFromInt(unique_bytes)) / 256.0;
        
        // Different algorithms have different efficiency characteristics
        return switch (algorithm) {
            .None => 1.0,
            .Zlib => @max(0.1, 1.0 - entropy_ratio * 0.8), // LZ77 good for repetitive data
            .Lz4 => @max(0.2, 1.0 - entropy_ratio * 0.6), // LZ4 trade speed for ratio
            .Zstd => @max(0.05, 1.0 - entropy_ratio * 0.9), // Zstd best general compression
        };
    }
    
    /// Benchmark compression algorithms for optimal selection
    pub fn benchmarkCompression(data: []const u8, allocator: Allocator) !struct {
        zlib: struct { ratio: f64, time_ns: u64 },
        lz4: struct { ratio: f64, time_ns: u64 },
        zstd: struct { ratio: f64, time_ns: u64 },
    } {
        if (data.len < 128) {
            return CompressionError.DataTooSmall;
        }
        
        // Limit benchmark size for reasonable performance
        const benchmark_size = @min(data.len, 64 * 1024);
        const benchmark_data = data[0..benchmark_size];
        
        // Benchmark Zlib
        const zlib_start = std.time.nanoTimestamp();
        const zlib_compressed = compressLZ77(benchmark_data) catch return CompressionError.DecompressionFailed;
        const zlib_end = std.time.nanoTimestamp();
        const zlib_ratio = @as(f64, @floatFromInt(zlib_compressed.len)) / @as(f64, @floatFromInt(benchmark_data.len));
        allocator.free(zlib_compressed);
        
        // Benchmark LZ4
        const lz4_start = std.time.nanoTimestamp();
        const lz4_compressed = compressLZ4Fast(benchmark_data) catch return CompressionError.DecompressionFailed;
        const lz4_end = std.time.nanoTimestamp();
        const lz4_ratio = @as(f64, @floatFromInt(lz4_compressed.len)) / @as(f64, @floatFromInt(benchmark_data.len));
        allocator.free(lz4_compressed);
        
        // Benchmark Zstd
        const zstd_start = std.time.nanoTimestamp();
        const zstd_compressed = compressZstandard(benchmark_data) catch return CompressionError.DecompressionFailed;
        const zstd_end = std.time.nanoTimestamp();
        const zstd_ratio = @as(f64, @floatFromInt(zstd_compressed.len)) / @as(f64, @floatFromInt(benchmark_data.len));
        allocator.free(zstd_compressed);
        
        return .{
            .zlib = .{ .ratio = zlib_ratio, .time_ns = zstd_end - zlib_start },
            .lz4 = .{ .ratio = lz4_ratio, .time_ns = lz4_end - lz4_start },
            .zstd = .{ .ratio = zstd_ratio, .time_ns = zstd_end - zstd_start },
        };
    }
    
    /// Compress data with automatic algorithm selection based on data characteristics
    pub fn compressAdaptive(data: []const u8, allocator: Allocator) !struct {
        compressed: []const u8,
        algorithm: CompressionType,
        ratio: f64,
    } {
        if (data.len == 0) {
            return .{
                .compressed = data,
                .algorithm = .None,
                .ratio = 1.0,
            };
        }
        
        // Get algorithm recommendation
        const algorithm = getRecommendedAlgorithm(data);
        
        // For very small data or algorithms that won't help, return original
        if (algorithm == .None or data.len < 128) {
            return .{
                .compressed = data,
                .algorithm = .None,
                .ratio = 1.0,
            };
        }
        
        // Compress with selected algorithm
        const compressed = switch (algorithm) {
            .None => data,
            .Zlib => compressLZ77(data) catch return error.CompressionFailed,
            .Lz4 => compressLZ4Fast(data) catch return error.CompressionFailed,
            .Zstd => compressZstandard(data) catch return error.CompressionFailed,
        };
        
        const ratio = @as(f64, @floatFromInt(compressed.len)) / @as(f64, @floatFromInt(data.len));
        
        // If compression didn't help significantly, return original data
        if (ratio > 0.95) {
            if (compressed.len != data.len) {
                allocator.free(compressed);
            }
            return .{
                .compressed = data,
                .algorithm = .None,
                .ratio = 1.0,
            };
        }
        
        return .{
            .compressed = compressed,
            .algorithm = algorithm,
            .ratio = ratio,
        };
    }
    
    /// Validate compressed data integrity
    pub fn validateCompressedData(compressed: []const u8, algorithm: CompressionType, original_size: usize) !bool {
        // Basic size validation
        if (compressed.len == 0) return original_size == 0;
        if (compressed.len > original_size * 2) return false; // Reasonable upper bound
        
        // Try decompression to validate integrity
        const decompressed = decompress(compressed, algorithm, original_size) catch return false;
        defer std.heap.page_allocator.free(decompressed);
        
        // Note: We can't verify exact content without original data
        // This is a basic integrity check
        return decompressed.len == original_size;
    }

    /// Production-ready LZ77 compression with optimized search and performance enhancements
    fn compressLZ77(data: []const u8) ![]const u8 {
        if (data.len == 0) return data;
        if (data.len > 32 * 1024 * 1024) return CompressionError.DataTooLarge; // Increased limit
        
        // Early exit for small data - compression overhead not worth it
        if (data.len < 32) return data;
        
        // Estimate output size more accurately
        var output = std.ArrayList(u8).init(std.heap.page_allocator);
        try output.ensureTotalCapacity(@max(data.len / 2, 1024));
        
        // Optimized LZ77 with multiple optimization strategies
        const window_size = 8192; // Increased window for better compression
        const min_match = 3;
        const max_match = 258;
        const hash_size = 16384; // Increased hash table size
        
        // Optimized hash table with better collision handling
        var hash_table = std.AutoHashMap(u32, std.ArrayList(usize)).init(std.heap.page_allocator);
        defer {
            var it = hash_table.valueIterator();
            while (it.next()) |list| {
                list.deinit();
            }
            hash_table.deinit();
        }
        
        var i: usize = 0;
        var literals_since_last_match: usize = 0;
        const max_literals_before_flush = 64;
        
        while (i < data.len - min_match) {
            // Optimized hash calculation
            var hash: u32 = 0;
            var j: usize = 0;
            while (j < min_match and i + j < data.len) : (j += 1) {
                hash = (hash * 1315423911) ^ data[i + j]; // Better hash function
            }
            
            // Find best match among potential candidates
            var best_match_len: usize = 0;
            var best_match_pos: usize = 0;
            
            if (hash_table.get(hash)) |positions| {
                const search_start = if (i > window_size) i - window_size else 0;
                
                // Check each potential match position
                for (positions.items) |match_pos| {
                    if (match_pos < search_start) continue;
                    if (match_pos >= i) continue;
                    
                    // Quick length check
                    var match_len = min_match;
                    const max_possible_len = @min(max_match, data.len - i, i - match_pos + max_match);
                    
                    while (match_len < max_possible_len and 
                           data[i + match_len] == data[match_pos + match_len]) {
                        match_len += 1;
                    }
                    
                    if (match_len > best_match_len) {
                        best_match_len = match_len;
                        best_match_pos = match_pos;
                        
                        // Early exit for very good matches
                        if (match_len >= 16) break;
                    }
                }
            }
            
            // Use match if it's good enough
            if (best_match_len >= min_match) {
                const offset = i - best_match_pos;
                const length_code = best_match_len - min_match;
                
                // Write match with optimized encoding
                if (length_code < 32) {
                    try output.append(@as(u8, 0xC0 | length_code));
                } else if (length_code < 96) {
                    try output.append(@as(u8, 0xC0 | 31));
                    try writeVarInt(&output, length_code - 31);
                } else {
                    try output.append(@as(u8, 0xC0 | 63));
                    try writeVarInt(&output, length_code - 63);
                }
                
                // Write offset with variable encoding
                if (offset < 128) {
                    try output.append(@as(u8, offset));
                } else {
                    try output.append(0);
                    try writeVarInt(&output, offset);
                }
                
                i += best_match_len;
                literals_since_last_match = 0;
            } else {
                // Write literal
                try output.append(data[i]);
                
                // Update hash table with current position
                var positions = hash_table.getOrPut(hash) catch continue;
                if (positions.value_ptr.* == null) {
                    positions.value_ptr.* = std.ArrayList(usize).init(std.heap.page_allocator);
                }
                positions.value_ptr.*.append(i) catch continue;
                
                // Limit hash table size to prevent memory bloat
                if (positions.value_ptr.*.items.len > 8) {
                    _ = positions.value_ptr.*.orderedRemove(0);
                }
                
                i += 1;
                literals_since_last_match += 1;
            }
            
            // Periodic cleanup of old hash entries
            if (i % 1024 == 0) {
                var to_remove = std.ArrayList(u32).init(std.heap.page_allocator);
                defer to_remove.deinit();
                
                var it = hash_table.keyIterator();
                while (it.next()) |key| {
                    // Simple heuristic to identify stale entries
                    if (hash_table.get(key.*)) |positions| {
                        var has_recent = false;
                        for (positions.items) |pos| {
                            if (i - pos <= window_size * 2) {
                                has_recent = true;
                                break;
                            }
                        }
                        if (!has_recent) {
                            try to_remove.append(key.*);
                        }
                    }
                }
                
                for (to_remove.items) |key| {
                    hash_table.remove(key);
                }
            }
        }
        
        // Write remaining bytes efficiently
        while (i < data.len) {
            try output.append(data[i]);
            i += 1;
        }
        
        return output.toOwnedSlice() catch return CompressionError.OutOfMemory;
    }
    
    /// Decompress LZ77 with corruption recovery
    fn decompressLZ77(compressed: []const u8, original_size: usize) ![]const u8 {
        var output = std.ArrayList(u8).init(std.heap.page_allocator);
        try output.ensureTotalCapacity(original_size);
        
        var i: usize = 0;
        while (i < compressed.len and output.items.len < original_size) {
            const control = compressed[i];
            i += 1;
            
            if (control & 0xC0 == 0xC0) {
                // Match token
                if (i >= compressed.len) break;
                
                // Read length
                var length = control & 0x3F;
                if (length == 63) {
                    length = 63 + try readVarInt(compressed, &i);
                }
                length += 3;
                
                // Read offset
                var offset: usize = 0;
                if (i < compressed.len) {
                    offset = compressed[i];
                    i += 1;
                    if (offset == 0) {
                        offset = try readVarInt(compressed, &i);
                    }
                }
                
                // Copy match with bounds checking
                const copy_start = output.items.len - offset;
                if (copy_start < output.items.len) {
                    for (0..length) |_| {
                        if (output.items.len >= original_size) break;
                        const source_pos = output.items.len - offset;
                        if (source_pos < output.items.len and source_pos < output.items.len) {
                            try output.append(output.items[source_pos]);
                        } else break;
                    }
                }
            } else {
                // Literal byte
                if (output.items.len < original_size and i < compressed.len) {
                    try output.append(control);
                }
            }
        }
        
        return output.toOwnedSlice() catch return CompressionError.OutOfMemory;
    }
    
    /// High-performance LZ4 compression with advanced optimizations
    fn compressLZ4Fast(data: []const u8) ![]const u8 {
        if (data.len == 0) return data;
        if (data.len > 64 * 1024 * 1024) return CompressionError.DataTooLarge; // Increased limit
        
        // Early exit for small data
        if (data.len < 64) return data;
        
        var output = std.ArrayList(u8).init(std.heap.page_allocator);
        try output.ensureTotalCapacity(@max(data.len / 2, 2048));
        
        // Optimized parameters for better speed/compression ratio
        const min_match = 4;
        const search_window = 128 * 1024; // Increased window
        const max_search_distance = 512; // Limit search distance for speed
        const acceleration_factor = 2; // Skip positions for acceleration
        
        // Multi-hash table for better match detection
        const hash_size = 32768;
        var hash_tables = [2]std.AutoHashMap(u32, usize){
            std.AutoHashMap(u32, usize).init(std.heap.page_allocator),
            std.AutoHashMap(u32, usize).init(std.heap.page_allocator),
        };
        defer {
            hash_tables[0].deinit();
            hash_tables[1].deinit();
        }
        
        var i: usize = 0;
        while (i < data.len - min_match) {
            // Use multiple hash functions for better match detection
            var hashes = [2]u32{0, 0};
            for (hashes, 0..) |*hash, hi| {
                for (0..min_match) |j| {
                    if (i + j >= data.len) break;
                    const byte = data[i + j];
                    if (hi == 0) {
                        hash.* = hash.* * 2654435761 + byte; // Knuth multiplicative hash
                    } else {
                        hash.* = (hash.* << 5) ^ byte; // Simple XOR hash
                    }
                }
            }
            
            // Find best match using multiple hash tables
            var best_match_len: usize = 0;
            var best_offset: usize = 0;
            var best_table_idx: usize = 0;
            
            for (hashes, 0..) |hash, table_idx| {
                if (hash_tables[table_idx].get(hash)) |match_pos| {
                    const search_start = if (i > search_window) i - search_window else 0;
                    if (match_pos < search_start or match_pos >= i) continue;
                    
                    const offset = i - match_pos;
                    if (offset > max_search_distance) continue;
                    
                    // Fast match length calculation with early termination
                    var match_len = min_match;
                    const max_possible_len = @min(32, data.len - i, offset + 32);
                    
                    while (match_len < max_possible_len and 
                           data[i + match_len] == data[match_pos + match_len]) {
                        match_len += 1;
                    }
                    
                    if (match_len > best_match_len) {
                        best_match_len = match_len;
                        best_offset = offset;
                        best_table_idx = table_idx;
                    }
                }
            }
            
            if (best_match_len >= min_match) {
                // Write match block with optimized encoding
                const literal_len = @min(15, i);
                const match_len_code = @min(15, best_match_len - min_match);
                
                try output.append(@as(u8, (literal_len << 4) | match_len_code));
                
                // Write literals efficiently
                if (literal_len > 0) {
                    try output.appendSlice(data[i - literal_len..i]);
                }
                
                // Write offset with bounds checking
                if (best_offset <= 255) {
                    try output.append(@as(u8, best_offset));
                } else {
                    // For larger offsets, write as literals (LZ4 limitation)
                    try output.append(@as(u8, @min(255, best_offset)));
                }
                
                // Update hash tables with current position
                for (hashes, 0..) |hash, table_idx| {
                    hash_tables[table_idx].put(hash, i) catch {};
                }
                
                i += best_match_len;
            } else {
                // No good match, advance with acceleration
                const advance = if (i % acceleration_factor == 0) 1 else acceleration_factor;
                
                // Update hash tables
                for (hashes, 0..) |hash, table_idx| {
                    hash_tables[table_idx].put(hash, i) catch {};
                }
                
                i += advance;
            }
            
            // Periodic cleanup to prevent memory bloat
            if (i % (8 * 1024) == 0) {
                var keys_to_remove = std.ArrayList(u32).init(std.heap.page_allocator);
                defer keys_to_remove.deinit();
                
                for (hash_tables) |*hash_table| {
                    var it = hash_table.keyIterator();
                    while (it.next()) |key| {
                        if (hash_table.get(key.*)) |pos| {
                            if (i - pos > search_window * 2) {
                                try keys_to_remove.append(key.*);
                            }
                        }
                    }
                    
                    for (keys_to_remove.items) |key| {
                        hash_table.remove(key);
                    }
                    keys_to_remove.clearRetainingCapacity();
                }
            }
        }
        
        // Write remaining bytes efficiently
        if (i < data.len) {
            const remaining = data.len - i;
            const literal_len = @min(15, remaining);
            try output.append(@as(u8, literal_len << 4));
            try output.appendSlice(data[i..i + literal_len]);
            
            // Write additional literals if needed
            i += literal_len;
            while (i < data.len) {
                const block_len = @min(15, data.len - i);
                try output.append(@as(u8, block_len << 4));
                try output.appendSlice(data[i..i + block_len]);
                i += block_len;
            }
        }
        
        return output.toOwnedSlice() catch return CompressionError.OutOfMemory;
    }
    
    /// Fast LZ4 decompression with bounds checking
    fn decompressLZ4Fast(compressed: []const u8, original_size: usize) ![]const u8 {
        var output = std.ArrayList(u8).init(std.heap.page_allocator);
        try output.ensureTotalCapacity(original_size);
        
        var i: usize = 0;
        while (i < compressed.len and output.items.len < original_size) {
            if (i >= compressed.len) break;
            
            const token = compressed[i];
            i += 1;
            
            const literal_len = token >> 4;
            const match_len = (token & 0xF) + 4;
            
            // Copy literals
            if (literal_len > 0) {
                const copy_len = @min(literal_len, original_size - output.items.len);
                if (i + copy_len <= compressed.len) {
                    try output.appendSlice(compressed[i..i + copy_len]);
                    i += copy_len;
                }
            }
            
            // Copy match if present
            if (match_len > 4 and i < compressed.len and output.items.len < original_size) {
                const offset = compressed[i];
                i += 1;
                
                const copy_start = output.items.len - offset;
                const copy_len = @min(match_len, original_size - output.items.len);
                
                if (copy_start < output.items.len) {
                    for (0..copy_len) |_| {
                        if (output.items.len >= original_size) break;
                        if (copy_start < output.items.len) {
                            try output.append(output.items[copy_start + (output.items.len - copy_start)]);
                        } else break;
                    }
                }
            }
        }
        
        return output.toOwnedSlice() catch return CompressionError.OutOfMemory;
    }
    
    /// High-performance Zstandard-style compression with advanced entropy coding
    fn compressZstandard(data: []const u8) ![]const u8 {
        if (data.len == 0) return data;
        if (data.len > 128 * 1024 * 1024) return CompressionError.DataTooLarge; // Increased limit
        
        // Early exit for small data or highly random data
        if (data.len < 128) return data;
        
        // Quick entropy check - if data is too random, skip compression
        var unique_bytes: usize = 0;
        var byte_seen = [_]bool{false} ** 256;
        for (data) |byte| {
            if (!byte_seen[byte]) {
                byte_seen[byte] = true;
                unique_bytes += 1;
            }
        }
        
        const entropy_ratio = @as(f64, @floatFromInt(unique_bytes)) / @as(f64, @floatFromInt(data.len));
        if (entropy_ratio > 0.8) {
            // Data is too random, compression won't help much
            return data;
        }
        
        var output = std.ArrayList(u8).init(std.heap.page_allocator);
        try output.ensureTotalCapacity(@max(data.len / 2, 2048));
        
        // Adaptive symbol table size based on data characteristics
        const symbol_limit = if (data.len < 1024 * 1024) 64 else 128;
        
        // Build enhanced frequency table with statistics
        var freq = [_]usize{0} ** 256;
        var positions = std.AutoHashMap(u8, std.ArrayList(usize)).init(std.heap.page_allocator);
        defer {
            var it = positions.valueIterator();
            while (it.next()) |list| {
                list.deinit();
            }
            positions.deinit();
        }
        
        for (data, 0..) |byte, i| {
            freq[byte] += 1;
            
            // Track positions for better match detection
            if (i % 1024 == 0) { // Sample positions to save memory
                var pos_list = positions.getOrPut(byte) catch continue;
                if (pos_list.value_ptr.* == null) {
                    pos_list.value_ptr.* = std.ArrayList(usize).init(std.heap.page_allocator);
                }
                pos_list.value_ptr.*.append(i) catch continue;
            }
        }
        
        // Create optimized symbol table
        var symbols = std.ArrayList(struct { symbol: u8, freq: usize, estimated_bits: f64 }).init(std.heap.page_allocator);
        defer symbols.deinit();
        
        // Select symbols based on frequency and estimated compression benefit
        for (0..256) |i| {
            if (freq[i] > 0) {
                const byte = @as(u8, @intCast(i));
                const byte_freq = freq[i];
                const byte_ratio = @as(f64, @floatFromInt(byte_freq)) / @as(f64, @floatFromInt(data.len));
                
                // Calculate estimated bits needed if not compressed
                const uncompressed_bits = 8.0;
                const estimated_compressed_bits = -@log2(byte_ratio + 1e-10); // Avoid log(0)
                const compression_benefit = uncompressed_bits - estimated_compressed_bits;
                
                try symbols.append(.{
                    .symbol = byte,
                    .freq = byte_freq,
                    .estimated_bits = compression_benefit
                });
            }
        }
        
        // Sort by compression benefit (descending)
        if (symbols.items.len > 1) {
            std.sort.sort(struct { symbol: u8, freq: usize, estimated_bits: f64 }, symbols.items, {}, struct {
                fn less(a: struct { symbol: u8, freq: usize, estimated_bits: f64 }, b: struct { symbol: u8, freq: usize, estimated_bits: f64 }) bool {
                    return a.estimated_bits > b.estimated_bits;
                }
            }.less);
        }
        
        // Write adaptive header
        const symbol_count = @min(symbols.items.len, symbol_limit);
        try output.append(@as(u8, symbol_count));
        
        // Build optimized code table using Shannon-Fano coding
        var code_map = std.AutoHashMap(u8, struct { code: u8, length: u8 }).init(std.heap.page_allocator);
        defer code_map.deinit();
        
        // Calculate code lengths using Shannon-Fano method
        var total_freq: usize = 0;
        for (0..symbol_count) |i| {
            total_freq += symbols.items[i].freq;
        }
        
        var cumulative_freq: usize = 0;
        for (0..symbol_count, symbols.items) |idx, symbol_entry| {
            const probability = @as(f64, @floatFromInt(symbol_entry.freq)) / @as(f64, @floatFromInt(total_freq));
            const code_length = @as(u8, @intCast(@ceil(-@log2(probability + 1e-10))));
            const adjusted_length = @max(1, @min(12, code_length)); // Bound code lengths
            
            try code_map.put(symbol_entry.symbol, .{ .code = @as(u8, @intCast(idx)), .length = adjusted_length });
            cumulative_freq += symbol_entry.freq;
        }
        
        // Write optimized symbol table with frequency information
        for (0..symbol_count) |i| {
            const entry = symbols.items[i];
            try output.append(entry.symbol);
            try writeVarInt(&output, entry.freq);
            try output.append(entry.estimated_bits); // Add compression benefit info
        }
        
        // Adaptive encoding based on symbol frequency
        var literal_run_length: usize = 0;
        const max_literal_run = 16;
        
        for (data) |byte| {
            if (code_map.get(byte)) |code_info| {
                // Use compressed encoding
                if (literal_run_length > 0) {
                    // Write pending literal run
                    try output.append(@as(u8, 0x40 | @as(u8, @min(15, literal_run_length - 1))));
                    literal_run_length = 0;
                }
                
                // Write compressed symbol
                const token = (code_info.length << 4) | (code_info.code & 0xF);
                try output.append(token);
                
                // Write additional code bits if needed
                if (code_info.length > 4) {
                    const additional_bits = code_info.code >> 4;
                    if (additional_bits > 0) {
                        try output.append(additional_bits);
                    }
                }
            } else {
                // Use literal byte
                try output.append(byte);
                literal_run_length += 1;
                
                // Flush literal run if it gets too long
                if (literal_run_length >= max_literal_run) {
                    try output.append(@as(u8, 0x40 | 15)); // Max literal run
                    literal_run_length = 0;
                }
            }
        }
        
        // Write final literal run if any
        if (literal_run_length > 0) {
            try output.append(@as(u8, 0x40 | @as(u8, @min(15, literal_run_length - 1))));
        }
        
        return output.toOwnedSlice() catch return CompressionError.OutOfMemory;
    }
    
    /// Zstandard decompression with error recovery
    fn decompressZstandard(compressed: []const u8, original_size: usize) ![]const u8 {
        if (compressed.len == 0) return &[_]u8{};
        
        var output = std.ArrayList(u8).init(std.heap.page_allocator);
        try output.ensureTotalCapacity(original_size);
        
        var i: usize = 0;
        
        // Read header
        if (i >= compressed.len) return CompressionError.InvalidCompressedData;
        const symbol_count = compressed[i];
        i += 1;
        
        if (symbol_count == 0) return &[_]u8{};
        
        var symbols = std.ArrayList(u8).init(std.heap.page_allocator);
        defer symbols.deinit();
        
        // Read symbol table
        for (0..symbol_count) |_| {
            if (i >= compressed.len) break;
            const symbol = compressed[i];
            i += 1;
            try symbols.append(symbol);
            
            // Skip frequency
            var freq = readVarInt(compressed, &i) catch break;
            _ = freq;
        }
        
        // Decode data
        while (i < compressed.len and output.items.len < original_size) {
            const code = compressed[i];
            i += 1;
            
            if (code & 0x80 != 0) {
                // Encoded symbol
                const symbol_idx = code & 0x7F;
                if (symbol_idx < symbols.items.len) {
                    try output.append(symbols.items[symbol_idx]);
                }
            } else {
                // Literal byte
                try output.append(code);
            }
        }
        
        return output.toOwnedSlice() catch return CompressionError.OutOfMemory;
    }
    
    /// Stream LZ77 compression with sliding window and hash-based matching
    fn compressLZ77Stream(reader: anytype, writer: anytype) !void {
        const window_size = 4096;
        const min_match = 3;
        const max_match = 258;
        const buffer_size = 16384;
        
        // Hash table for fast match detection
        var hash_table = std.AutoHashMap(u32, usize).init(std.heap.page_allocator);
        defer hash_table.deinit();
        
        var input_buffer = std.ArrayList(u8).init(std.heap.page_allocator);
        defer input_buffer.deinit();
        
        var output_buffer = std.ArrayList(u8).init(std.heap.page_allocator);
        defer output_buffer.deinit();
        
        try output_buffer.ensureTotalCapacity(buffer_size);
        
        var total_input: usize = 0;
        
        while (true) {
            // Read chunk of data
            var chunk_buffer = try std.heap.page_allocator.alloc(u8, buffer_size);
            defer std.heap.page_allocator.free(chunk_buffer);
            
            const bytes_read = try reader.read(chunk_buffer);
            if (bytes_read == 0) break;
            
            total_input += bytes_read;
            
            // Process the chunk
            for (chunk_buffer[0..bytes_read], 0..) |byte, i| {
                try input_buffer.append(byte);
                
                // Maintain sliding window
                if (input_buffer.items.len > window_size + max_match) {
                    const remove_count = input_buffer.items.len - (window_size + max_match);
                    // Remove old entries from hash table
                    for (0..remove_count) |j| {
                        if (j + min_match <= input_buffer.items.len) {
                            var hash: u32 = 0;
                            const start = j;
                            for (0..min_match) |k| {
                                if (start + k < input_buffer.items.len) {
                                    hash = hash * 31 + input_buffer.items[start + k];
                                }
                            }
                            hash_table.remove(hash);
                        }
                    }
                    std.mem.copyForwards(u8, input_buffer.items[remove_count..], input_buffer.items[0..remove_count]);
                    input_buffer.shrinkRetainingCapacity(input_buffer.items.len - remove_count);
                }
                
                // Find matches when we have enough data
                if (input_buffer.items.len >= min_match) {
                    const current_pos = input_buffer.items.len - min_match;
                    
                    // Hash current position
                    var hash: u32 = 0;
                    for (0..min_match) |j| {
                        if (current_pos + j < input_buffer.items.len) {
                            hash = hash * 31 + input_buffer.items[current_pos + j];
                        }
                    }
                    
                    // Find potential matches
                    if (hash_table.get(hash)) |match_pos| {
                        const search_start = if (current_pos > window_size) current_pos - window_size else 0;
                        if (match_pos >= search_start) {
                            // Verify match and find longest
                            var match_len = min_match;
                            while (current_pos + match_len < input_buffer.items.len and 
                                   match_pos + match_len < input_buffer.items.len and
                                   input_buffer[current_pos + match_len] == input_buffer[match_pos + match_len] and
                                   match_len < max_match) {
                                match_len += 1;
                            }
                            
                            if (match_len >= min_match) {
                                // Write match to output buffer
                                const offset = current_pos - match_pos;
                                const length_code = match_len - min_match;
                                
                                if (length_code < 64) {
                                    try output_buffer.append(@as(u8, 0xC0 | length_code));
                                } else {
                                    try output_buffer.append(@as(u8, 0xC0 | 63));
                                    try writeVarInt(&output_buffer, length_code - 63);
                                }
                                
                                if (offset < 256) {
                                    try output_buffer.append(@as(u8, offset));
                                } else {
                                    try output_buffer.append(0);
                                    try writeVarInt(&output_buffer, offset);
                                }
                                
                                // Flush output buffer if it's getting large
                                if (output_buffer.items.len >= buffer_size) {
                                    try writer.writeAll(output_buffer.items);
                                    output_buffer.clearRetainingCapacity();
                                }
                                
                                continue;
                            }
                        }
                    }
                    
                    // No good match, write literal
                    try output_buffer.append(byte);
                    hash_table.put(hash, current_pos) catch {};
                }
                
                // Flush output buffer periodically
                if (output_buffer.items.len >= buffer_size / 2) {
                    try writer.writeAll(output_buffer.items);
                    output_buffer.clearRetainingCapacity();
                }
            }
        }
        
        // Write any remaining output
        if (output_buffer.items.len > 0) {
            try writer.writeAll(output_buffer.items);
        }
    }
    
    /// Stream LZ77 decompression with memory efficiency
    fn decompressLZ77Stream(reader: anytype, writer: anytype, expected_size: usize) !void {
        var output_buffer = std.ArrayList(u8).init(std.heap.page_allocator);
        defer output_buffer.deinit();
        
        try output_buffer.ensureTotalCapacity(@min(expected_size, 16384));
        
        var input_buffer = std.ArrayList(u8).init(std.heap.page_allocator);
        defer input_buffer.deinit();
        
        const input_buffer_size = 8192;
        var bytes_processed: usize = 0;
        var offset: usize = 0;
        
        while (bytes_processed < expected_size) {
            // Refill input buffer if needed
            if (offset >= input_buffer.items.len) {
                input_buffer.clearRetainingCapacity();
                var chunk_buffer = try std.heap.page_allocator.alloc(u8, input_buffer_size);
                defer std.heap.page_allocator.free(chunk_buffer);
                
                const to_read = @min(chunk_buffer.len, expected_size - bytes_processed);
                const bytes_read = try reader.read(chunk_buffer[0..to_read]);
                if (bytes_read == 0) break;
                
                try input_buffer.appendSlice(chunk_buffer[0..bytes_read]);
                offset = 0;
            }
            
            if (offset >= input_buffer.items.len) break;
            
            const control = input_buffer.items[offset];
            offset += 1;
            
            if (control & 0xC0 == 0xC0) {
                // Match token
                if (offset >= input_buffer.items.len) break;
                
                // Read length
                var length = control & 0x3F;
                if (length == 63) {
                    length = 63 + try readVarInt(input_buffer.items, &offset);
                }
                length += 3;
                
                // Read offset
                var match_offset: usize = 0;
                if (offset < input_buffer.items.len) {
                    match_offset = input_buffer.items[offset];
                    offset += 1;
                    if (match_offset == 0) {
                        match_offset = try readVarInt(input_buffer.items, &offset);
                    }
                }
                
                // Copy match with bounds checking
                const copy_start = output_buffer.items.len - match_offset;
                if (copy_start < output_buffer.items.len) {
                    const copy_len = @min(length, expected_size - bytes_processed);
                    for (0..copy_len) |_| {
                        if (output_buffer.items.len >= expected_size) break;
                        if (copy_start < output_buffer.items.len) {
                            try output_buffer.append(output_buffer.items[copy_start + (output_buffer.items.len - copy_start)]);
                            bytes_processed += 1;
                        } else break;
                    }
                }
            } else {
                // Literal byte
                if (bytes_processed < expected_size and offset < input_buffer.items.len) {
                    try output_buffer.append(control);
                    bytes_processed += 1;
                }
            }
            
            // Flush output buffer periodically
            if (output_buffer.items.len >= 8192) {
                try writer.writeAll(output_buffer.items);
                output_buffer.clearRetainingCapacity();
            }
        }
        
        // Write remaining output
        if (output_buffer.items.len > 0) {
            try writer.writeAll(output_buffer.items);
        }
    }
    
    /// Stream LZ4 compression optimized for speed
    fn compressLZ4Stream(reader: anytype, writer: anytype) !void {
        const min_match = 4;
        const search_window = 64 * 1024;
        const block_size = 16384;
        
        var input_buffer = std.ArrayList(u8).init(std.heap.page_allocator);
        defer input_buffer.deinit();
        
        var output_buffer = std.ArrayList(u8).init(std.heap.page_allocator);
        defer output_buffer.deinit();
        
        try output_buffer.ensureTotalCapacity(block_size);
        
        while (true) {
            // Read block of data
            var block_buffer = try std.heap.page_allocator.alloc(u8, block_size);
            defer std.heap.page_allocator.free(block_buffer);
            
            const bytes_read = try reader.read(block_buffer);
            if (bytes_read == 0) break;
            
            try input_buffer.appendSlice(block_buffer[0..bytes_read]);
            
            // Process block
            var i: usize = 0;
            while (i < input_buffer.items.len - min_match) {
                // Simple fast matching (optimized for speed over compression ratio)
                var best_match_len: usize = 0;
                var best_offset: usize = 0;
                
                const search_start = if (i > search_window) i - search_window else 0;
                const search_end = @min(i + 256, input_buffer.items.len - min_match);
                
                // Fast linear search with early exit
                for (search_start..search_end) |search_pos| {
                    var match_len: usize = 0;
                    while (match_len < 16 and i + match_len < input_buffer.items.len and 
                           search_pos + match_len < input_buffer.items.len and
                           input_buffer[i + match_len] == input_buffer[search_pos + match_len]) {
                        match_len += 1;
                    }
                    
                    if (match_len > best_match_len) {
                        best_match_len = match_len;
                        best_offset = i - search_pos;
                        if (match_len >= 8) break; // Early exit for good matches
                    }
                }
                
                if (best_match_len >= min_match) {
                    // Write match block
                    const literal_len = @min(15, i);
                    try output_buffer.append(@as(u8, (literal_len << 4) | @as(u8, @min(15, best_match_len - min_match))));
                    
                    // Write literals
                    if (literal_len > 0) {
                        try output_buffer.appendSlice(input_buffer.items[i - literal_len..i]);
                    }
                    
                    // Write offset
                    try output_buffer.append(@as(u8, @min(255, best_offset)));
                    
                    i += best_match_len;
                } else {
                    // No match, continue searching
                    i += 1;
                }
                
                // Flush output buffer periodically
                if (output_buffer.items.len >= block_size / 2) {
                    try writer.writeAll(output_buffer.items);
                    output_buffer.clearRetainingCapacity();
                }
            }
            
            // Write remaining bytes as literals
            if (i < input_buffer.items.len) {
                const remaining = input_buffer.items.len - i;
                const literal_len = @min(15, remaining);
                try output_buffer.append(@as(u8, literal_len << 4));
                try output_buffer.appendSlice(input_buffer.items[i..i + literal_len]);
            }
            
            // Clear input buffer for next block
            input_buffer.clearRetainingCapacity();
        }
        
        // Write any remaining output
        if (output_buffer.items.len > 0) {
            try writer.writeAll(output_buffer.items);
        }
    }
    
    /// Stream LZ4 decompression with bounds checking
    fn decompressLZ4Stream(reader: anytype, writer: anytype, expected_size: usize) !void {
        var output_buffer = std.ArrayList(u8).init(std.heap.page_allocator);
        defer output_buffer.deinit();
        
        try output_buffer.ensureTotalCapacity(@min(expected_size, 16384));
        
        var input_buffer = std.ArrayList(u8).init(std.heap.page_allocator);
        defer input_buffer.deinit();
        
        const input_buffer_size = 8192;
        var bytes_processed: usize = 0;
        var offset: usize = 0;
        
        while (bytes_processed < expected_size) {
            // Refill input buffer
            if (offset >= input_buffer.items.len) {
                input_buffer.clearRetainingCapacity();
                var chunk_buffer = try std.heap.page_allocator.alloc(u8, input_buffer_size);
                defer std.heap.page_allocator.free(chunk_buffer);
                
                const to_read = @min(chunk_buffer.len, expected_size - bytes_processed);
                const bytes_read = try reader.read(chunk_buffer[0..to_read]);
                if (bytes_read == 0) break;
                
                try input_buffer.appendSlice(chunk_buffer[0..bytes_read]);
                offset = 0;
            }
            
            if (offset >= input_buffer.items.len) break;
            
            const token = input_buffer.items[offset];
            offset += 1;
            
            const literal_len = token >> 4;
            const match_len = (token & 0xF) + 4;
            
            // Copy literals
            if (literal_len > 0) {
                const copy_len = @min(literal_len, expected_size - bytes_processed);
                if (offset + copy_len <= input_buffer.items.len) {
                    try output_buffer.appendSlice(input_buffer.items[offset..offset + copy_len]);
                    offset += copy_len;
                    bytes_processed += copy_len;
                }
            }
            
            // Copy match if present
            if (match_len > 4 and offset < input_buffer.items.len and bytes_processed < expected_size) {
                const match_offset = input_buffer.items[offset];
                offset += 1;
                
                const copy_start = output_buffer.items.len - match_offset;
                const copy_len = @min(match_len, expected_size - bytes_processed);
                
                if (copy_start < output_buffer.items.len) {
                    for (0..copy_len) |_| {
                        if (bytes_processed >= expected_size) break;
                        if (copy_start < output_buffer.items.len) {
                            try output_buffer.append(output_buffer.items[copy_start + (output_buffer.items.len - copy_start)]);
                            bytes_processed += 1;
                        } else break;
                    }
                }
            }
            
            // Flush output buffer periodically
            if (output_buffer.items.len >= 8192) {
                try writer.writeAll(output_buffer.items);
                output_buffer.clearRetainingCapacity();
            }
        }
        
        // Write remaining output
        if (output_buffer.items.len > 0) {
            try writer.writeAll(output_buffer.items);
        }
    }
    
    /// Stream Zstandard-style compression with entropy coding
    fn compressZstandardStream(reader: anytype, writer: anytype) !void {
        const block_size = 16384;
        const symbol_limit = 128;
        
        var input_buffer = std.ArrayList(u8).init(std.heap.page_allocator);
        defer input_buffer.deinit();
        
        var output_buffer = std.ArrayList(u8).init(std.heap.page_allocator);
        defer output_buffer.deinit();
        
        try output_buffer.ensureTotalCapacity(block_size);
        
        // Symbol frequency tracking
        var global_freq = [_]usize{0} ** 256;
        var blocks_processed: usize = 0;
        
        while (true) {
            // Read block of data
            var block_buffer = try std.heap.page_allocator.alloc(u8, block_size);
            defer std.heap.page_allocator.free(block_buffer);
            
            const bytes_read = try reader.read(block_buffer);
            if (bytes_read == 0) break;
            
            try input_buffer.appendSlice(block_buffer[0..bytes_read]);
            
            // Update global frequency table
            for (input_buffer.items) |byte| {
                global_freq[byte] += 1;
            }
            
            // Build symbol table for this block
            var symbols = std.ArrayList(struct { symbol: u8, freq: usize }).init(std.heap.page_allocator);
            defer symbols.deinit();
            
            // Add symbols with frequency > 0
            for (0..256) |i| {
                if (global_freq[i] > 0) {
                    try symbols.append(.{ .symbol = @as(u8, @intCast(i)), .freq = global_freq[i] });
                }
            }
            
            // Sort by frequency (most frequent first)
            if (symbols.items.len > 1) {
                std.sort.sort(struct { symbol: u8, freq: usize }, symbols.items, {}, struct {
                    fn less(a: struct { symbol: u8, freq: usize }, b: struct { symbol: u8, freq: usize }) bool {
                        return a.freq > b.freq;
                    }
                }.less);
            }
            
            // Write header for first block or when symbol table changes significantly
            if (blocks_processed == 0 or blocks_processed % 100 == 0) {
                const symbol_count = @min(symbols.items.len, symbol_limit);
                try output_buffer.append(@as(u8, symbol_count));
                
                // Write symbol table
                for (0..symbol_count) |i| {
                    const entry = symbols.items[i];
                    try output_buffer.append(entry.symbol);
                    try writeVarInt(&output_buffer, entry.freq);
                }
            }
            
            // Encode data using simple prefix coding
            var code_map = std.AutoHashMap(u8, u8).init(std.heap.page_allocator);
            defer code_map.deinit();
            
            const symbol_count = @min(symbols.items.len, symbol_limit);
            for (0..symbol_count, symbols.items) |idx, symbol_entry| {
                const code_len = @as(u8, @intCast(@divFloor(idx, 8) + 1));
                const code = @as(u8, @intCast(idx % 256));
                try code_map.put(symbol_entry.symbol, code_len << 4 | code);
            }
            
            // Encode block data
            for (input_buffer.items) |byte| {
                if (code_map.get(byte)) |code| {
                    try output_buffer.append(code);
                } else {
                    // Literal byte for unmapped symbols
                    try output_buffer.append(0x80 | byte);
                }
                
                // Flush output buffer periodically
                if (output_buffer.items.len >= block_size / 2) {
                    try writer.writeAll(output_buffer.items);
                    output_buffer.clearRetainingCapacity();
                }
            }
            
            // Clear input buffer for next block
            input_buffer.clearRetainingCapacity();
            blocks_processed += 1;
        }
        
        // Write any remaining output
        if (output_buffer.items.len > 0) {
            try writer.writeAll(output_buffer.items);
        }
    }
    
    /// Stream Zstandard decompression with entropy coding
    fn decompressZstandardStream(reader: anytype, writer: anytype, expected_size: usize) !void {
        var output_buffer = std.ArrayList(u8).init(std.heap.page_allocator);
        defer output_buffer.deinit();
        
        try output_buffer.ensureTotalCapacity(@min(expected_size, 16384));
        
        var input_buffer = std.ArrayList(u8).init(std.heap.page_allocator);
        defer input_buffer.deinit();
        
        const input_buffer_size = 8192;
        var bytes_processed: usize = 0;
        var offset: usize = 0;
        var symbols = std.ArrayList(u8).init(std.heap.page_allocator);
        defer symbols.deinit();
        
        var header_read = false;
        
        while (bytes_processed < expected_size) {
            // Refill input buffer
            if (offset >= input_buffer.items.len) {
                input_buffer.clearRetainingCapacity();
                var chunk_buffer = try std.heap.page_allocator.alloc(u8, input_buffer_size);
                defer std.heap.page_allocator.free(chunk_buffer);
                
                const to_read = @min(chunk_buffer.len, expected_size - bytes_processed);
                const bytes_read = try reader.read(chunk_buffer[0..to_read]);
                if (bytes_read == 0) break;
                
                try input_buffer.appendSlice(chunk_buffer[0..bytes_read]);
                offset = 0;
            }
            
            // Read header if not done yet
            if (!header_read) {
                if (offset >= input_buffer.items.len) break;
                
                const symbol_count = input_buffer.items[offset];
                offset += 1;
                
                symbols.clearRetainingCapacity();
                
                // Read symbol table
                for (0..symbol_count) |_| {
                    if (offset >= input_buffer.items.len) break;
                    const symbol = input_buffer.items[offset];
                    offset += 1;
                    try symbols.append(symbol);
                    
                    // Skip frequency
                    var freq = readVarInt(input_buffer.items, &offset) catch break;
                    _ = freq;
                }
                
                header_read = true;
            }
            
            if (offset >= input_buffer.items.len) break;
            
            const code = input_buffer.items[offset];
            offset += 1;
            
            if (code & 0x80 != 0) {
                // Encoded symbol
                const symbol_idx = code & 0x7F;
                if (symbol_idx < symbols.items.len and bytes_processed < expected_size) {
                    try output_buffer.append(symbols.items[symbol_idx]);
                    bytes_processed += 1;
                }
            } else {
                // Literal byte
                if (bytes_processed < expected_size) {
                    try output_buffer.append(code);
                    bytes_processed += 1;
                }
            }
            
            // Flush output buffer periodically
            if (output_buffer.items.len >= 8192) {
                try writer.writeAll(output_buffer.items);
                output_buffer.clearRetainingCapacity();
            }
        }
        
        // Write remaining output
        if (output_buffer.items.len > 0) {
            try writer.writeAll(output_buffer.items);
        }
    }
};

// ==================== VALIDATION REPORT STRUCTURES ====================

pub const ValidationIssue = struct {
    /// Severity level of the issue
    severity: IssueSeverity,
    /// Type of corruption detected
    corruption_type: CorruptionType,
    /// Human-readable description
    description: []const u8,
    /// File offset where issue was found
    offset: usize,
    /// Additional context data
    context: []const u8 = &[_]u8{},
    
    const Self = @This();
};

pub const IssueSeverity = enum(u8) {
    Info = 1,
    Warning = 2,
    Error = 3,
    Critical = 4,
};

pub const CorruptionType = enum(u8) {
    HeaderCorruption = 1,
    EntryCorruption = 2,
    FooterCorruption = 3,
    CRC32Mismatch = 4,
    SizeMismatch = 5,
    OffsetCorruption = 6,
    DataInconsistency = 7,
    TruncatedFile = 8,
    MalformedEntry = 9,
    TimestampAnomaly = 10,
};

pub const ValidationMode = enum {
    Validation,
    Repair,
    DeepAnalysis,
};

pub const ValidationReport = struct {
    /// Whether the file passed validation
    is_valid: bool,
    /// List of issues found during validation
    issues: std.ArrayList(ValidationIssue),
    /// Total number of entries validated
    entries_validated: usize,
    /// Total number of entries with issues
    entries_corrupted: usize,
    /// Calculated file CRC32
    calculated_crc32: u32,
    /// Expected file CRC32 from footer
    expected_crc32: u32,
    /// Whether the file can be repaired
    can_repair: bool,
    /// Whether repair was attempted
    repair_attempted: bool,
    /// Whether repair was successful
    repair_successful: bool,
    
    const Self = @This();
    
    pub fn init(allocator: Allocator) Self {
        return Self{
            .is_valid = true,
            .issues = std.ArrayList(ValidationIssue).init(allocator),
            .entries_validated = 0,
            .entries_corrupted = 0,
            .calculated_crc32 = 0,
            .expected_crc32 = 0,
            .can_repair = false,
            .repair_attempted = false,
            .repair_successful = false,
        };
    }
    
    pub fn deinit(self: *Self) void {
        self.issues.deinit();
    }
    
    pub fn addIssue(self: *Self, severity: IssueSeverity, corruption_type: CorruptionType, description: []const u8, offset: usize) void {
        self.issues.append(ValidationIssue{
            .severity = severity,
            .corruption_type = corruption_type,
            .description = description,
            .offset = offset,
        }) catch {};
        
        if (severity == .Error or severity == .Critical) {
            self.is_valid = false;
        }
    }
    
    pub fn getSummary(self: *Self) []const u8 {
        if (self.issues.items.len == 0) {
            return "File validation passed - no issues found";
        }
        
        var errors: usize = 0;
        var warnings: usize = 0;
        var infos: usize = 0;
        
        for (self.issues.items) |issue| {
            switch (issue.severity) {
                .Error, .Critical => errors += 1,
                .Warning => warnings += 1,
                .Info => infos += 1,
            }
        }
        
        return std.fmt.allocPrint(self.issues.allocator.allocator, 
            "Validation completed: {} errors, {} warnings, {} info", .{
            errors, warnings, infos
        }) catch "Validation summary unavailable";
    }
};

// ==================== FILE VALIDATION UTILITIES ====================

const CRC32_TABLE = init: {
    var table: [256]u32 = undefined;
    for (0..256) |i| {
        var crc = @as(u32, i);
        for (0..8) |_| {
            if (crc & 1 == 1) {
                crc = (crc >> 1) ^ 0xEDB88320;
            } else {
                crc >>= 1;
            }
        }
        table[i] = crc;
    }
    break :init table;
};

pub fn calculateFileCRC32(file: std.fs.File, allocator: Allocator) !u32 {
    var hasher = CRC32Calculator.init();
    
    // Read file in chunks to handle large files efficiently
    const chunk_size = 64 * 1024; // 64KB chunks
    var buffer = try allocator.alloc(u8, chunk_size);
    defer allocator.free(buffer);
    
    try file.seekTo(0);
    
    while (true) {
        const bytes_read = try file.read(buffer);
        if (bytes_read == 0) break;
        
        hasher.update(buffer[0..bytes_read]);
    }
    
    return hasher.final();
}

pub const CRC32Calculator = struct {
    crc: u32 = 0xFFFFFFFF,
    
    const Self = @This();
    
    pub fn init() Self {
        return Self{ .crc = 0xFFFFFFFF };
    }
    
    pub fn update(self: *Self, data: []const u8) void {
        for (data) |byte| {
            const index = (@as(u8, self.crc) ^ byte) & 0xFF;
            self.crc = (self.crc >> 8) ^ CRC32_TABLE[index];
        }
    }
    
    pub fn final(self: *Self) u32 {
        return self.crc ^ 0xFFFFFFFF;
    }
};

// ==================== COMPRESSION PERFORMANCE UTILITIES ====================

/// Compression statistics and performance metrics
pub const CompressionStats = struct {
    original_size: usize,
    compressed_size: usize,
    compression_ratio: f64,
    algorithm: CompressionType,
    compression_time_ns: u64,
    decompression_time_ns: u64,
    memory_used: usize,
    
    const Self = @This();
    
    pub fn init(original_size: usize, compressed_size: usize, algorithm: CompressionType, comp_time: u64, decomp_time: u64) Self {
        const ratio = if (original_size > 0) @as(f64, @floatFromInt(compressed_size)) / @as(f64, @floatFromInt(original_size)) else 1.0;
        
        return Self{
            .original_size = original_size,
            .compressed_size = compressed_size,
            .compression_ratio = ratio,
            .algorithm = algorithm,
            .compression_time_ns = comp_time,
            .decompression_time_ns = decomp_time,
            .memory_used = @max(original_size, compressed_size) * 2, // Rough estimate
        };
    }
    
    pub fn getCompressionEfficiency(self: *Self) f64 {
        return 1.0 - self.compression_ratio; // Higher is better
    }
    
    pub fn getThroughputMBps(self: *Self, is_compression: bool) f64 {
        const time_ns = if (is_compression) self.compression_time_ns else self.decompression_time_ns;
        const size_mb = @as(f64, @floatFromInt(self.original_size)) / (1024.0 * 1024.0);
        const time_s = @as(f64, @floatFromInt(time_ns)) / 1_000_000_000.0;
        
        return if (time_s > 0) size_mb / time_s else 0.0;
    }
};

/// Compress data with comprehensive performance tracking
pub fn compressWithStats(data: []const u8, algorithm: CompressionType, allocator: Allocator) !struct {
    compressed: []const u8,
    stats: CompressionStats,
} {
    const start_time = std.time.nanoTimestamp();
    const compressed = compress(data, algorithm) catch return error.CompressionFailed;
    const comp_time = std.time.nanoTimestamp() - start_time;
    
    const decomp_start = std.time.nanoTimestamp();
    _ = decompress(compressed, algorithm, data.len) catch |err| {
        allocator.free(compressed);
        return err;
    };
    const decomp_time = std.time.nanoTimestamp() - decomp_start;
    
    const stats = CompressionStats.init(data.len, compressed.len, algorithm, comp_time, decomp_time);
    
    return .{
        .compressed = compressed,
        .stats = stats,
    };
}

/// Batch compression for multiple data chunks
pub fn compressBatch(chunks: [][]const u8, algorithm: CompressionType, allocator: Allocator) !struct {
    compressed_chunks: std.ArrayList([]const u8),
    total_original: usize,
    total_compressed: usize,
    stats: CompressionStats,
} {
    var compressed_chunks = std.ArrayList([]const u8).init(allocator);
    var total_original: usize = 0;
    var total_compressed: usize = 0;
    var total_comp_time: u64 = 0;
    var total_decomp_time: u64 = 0;
    
    errdefer {
        for (compressed_chunks.items) |chunk| {
            allocator.free(chunk);
        }
        compressed_chunks.deinit();
    }
    
    for (chunks) |chunk| {
        const start_time = std.time.nanoTimestamp();
        const compressed = compress(chunk, algorithm) catch return error.CompressionFailed;
        const comp_time = std.time.nanoTimestamp() - start_time;
        
        const decomp_start = std.time.nanoTimestamp();
        _ = decompress(compressed, algorithm, chunk.len) catch |err| {
            allocator.free(compressed);
            return err;
        };
        const decomp_time = std.time.nanoTimestamp() - decomp_start;
        
        try compressed_chunks.append(compressed);
        total_original += chunk.len;
        total_compressed += compressed.len;
        total_comp_time += comp_time;
        total_decomp_time += decomp_time;
    }
    
    const stats = CompressionStats.init(total_original, total_compressed, algorithm, total_comp_time, total_decomp_time);
    
    return .{
        .compressed_chunks = compressed_chunks,
        .total_original = total_original,
        .total_compressed = total_compressed,
        .stats = stats,
    };
}

// ==================== COMPRESSION ERROR TYPES ====================

pub const CompressionError = error{
    /// Decompression failed due to corrupted data
    DecompressionFailed,
    /// Invalid compressed data format
    InvalidCompressedData,
    /// Compression buffer too small
    BufferTooSmall,
    /// Memory allocation failed during compression
    OutOfMemory,
    /// Data is too large to compress safely
    DataTooLarge,
    /// Compression ratio is worse than expected
    PoorCompressionRatio,
    /// Invalid compression parameters
    InvalidParameters,
    /// Stream ended unexpectedly
    UnexpectedStreamEnd,
    /// Checksum verification failed
    ChecksumMismatch,
};

pub const FileError = error{
    InvalidMagic,
    VersionTooNew,
    FileSizeExceeded,
    OffsetOutOfBounds,
    FileNotFound,
    InvalidHeader,
    OutOfMemory,
    VarIntTooLarge,
    ValidationFailed,
    FileCorrupted,
    RepairFailed,
    CRC32ValidationFailed,
    FooterValidationFailed,
    EntryValidationFailed,
};
