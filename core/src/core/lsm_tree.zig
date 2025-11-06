const std = @import("std");
const Allocator = std.mem.Allocator;
const os = std.os;
const File = std.fs.File;
const testing = std.testing;
const bdb_format = @import("bdb_format.zig");
const HeatMap = @import("heatmap_indexing.zig");

/// BrowserDB LSM-Tree Core Engine
/// 
/// This module provides the complete storage engine for BrowserDB, implementing
/// a high-performance LSM-Tree hybrid architecture with automatic compaction,
/// memory management, and multi-level storage organization.
/// 
/// Performance targets:
/// - Write: 15,000+ ops/sec (Persistent mode)
/// - Read: 25,000+ ops/sec (Persistent mode)
/// - Memory: <64MB per MemTable
/// - Compaction: 100MB+/sec processing speed

pub const VERSION = "1.0.0";

// ==================== CORE TYPES AND STRUCTURES ====================

/// Key-Value entry for storage operations
pub const KVEntry = struct {
    key: []const u8,
    value: []const u8,
    timestamp: u64,
    entry_type: EntryType,
    deleted: bool = false,
    heat: f32 = 0.0,

    const Self = @This();

    pub fn init(key: []const u8, value: []const u8, entry_type: EntryType) Self {
        return Self{
            .key = key,
            .value = value,
            .timestamp = std.time.milliTimestamp(),
            .entry_type = entry_type,
            .deleted = entry_type == .Delete,
            .heat = 0.0,
        };
    }

    pub fn getSize(self: *const Self) usize {
        return self.key.len + self.value.len + @sizeOf(u64) + @sizeOf(EntryType);
    }
};

/// Entry types for LSM-Tree operations
pub const EntryType = enum(u8) {
    Insert = 1,
    Update = 2,
    Delete = 3,
    Tombstone = 4,
};

/// SSTable block structure for on-disk storage
pub const SSTableBlock = struct {
    data_start: usize,
    data_end: usize,
    index_start: usize,
    index_end: usize,
    bloom_filter_offset: usize,
    compression_type: bdb_format.CompressionType,
    checksum: u32,

    const Self = @This();
};

/// Index entry for binary search in SSTables
pub const IndexEntry = struct {
    key: []const u8,
    position: usize,
    size: usize,
    timestamp: u64,

    const Self = @This();

    pub fn less(_: void, a: Self, b: Self) bool {
        return std.mem.order(u8, a.key, b.key) == .lt;
    }
};

/// Memory-mapped file for efficient disk I/O
pub const MemoryMappedFile = struct {
    fd: os.fd_t,
    ptr: *anyopaque,
    size: usize,
    path: []const u8,
    is_readonly: bool,
    allocator: Allocator,

    const Self = @This();

    /// Create a new memory-mapped file
    pub fn create(allocator: Allocator, path: []const u8, size: usize, is_readonly: bool) !*Self {
        const file = try allocator.create(Self);
        
        const flags: c_int = if (is_readonly) os.O.RDONLY else (os.O.RDWR | os.O.CREAT);
        const mode: c_int = if (is_readonly) 0 else 0o644;
        
        const fd = os.open(path, flags, mode);
        if (fd < 0) {
            return error.FileOpenFailed;
        }

        // Extend file if needed
        if (!is_readonly and size > 0) {
            if (os.ftruncate(fd, size) < 0) {
                return error.FileTruncateFailed;
            }
        }

        // Memory map the file
        const prot: c_int = if (is_readonly) os.PROT_READ else (os.PROT_READ | os.PROT_WRITE);
        const flags_map: c_int = os.MAP_SHARED;

        const ptr = os.mmap(null, size, prot, flags_map, fd, 0);
        if (ptr == os.MAP_FAILED) {
            return error.MemoryMapFailed;
        }

        file.* = .{
            .fd = fd,
            .ptr = ptr,
            .size = size,
            .path = path,
            .is_readonly = is_readonly,
            .allocator = allocator,
        };

        return file;
    }

    /// Read data from memory-mapped file
    pub fn read(self: *Self, offset: usize, length: usize) ![]const u8 {
        if (offset + length > self.size) {
            return error.ReadBeyondFile;
        }
        return @as([*]const u8, @ptrCast(self.ptr))[offset..offset + length];
    }

    /// Write data to memory-mapped file
    pub fn write(self: *Self, offset: usize, data: []const u8) !void {
        if (self.is_readonly) {
            return error.WriteToReadOnlyFile;
        }
        if (offset + data.len > self.size) {
            return error.WriteBeyondFile;
        }
        @as([*]u8, @ptrCast(self.ptr))[offset..offset + data.len].copyFromSlice(data);
    }

    /// Synchronize memory-mapped file to disk
    pub fn sync(self: *Self) !void {
        if (self.is_readonly) return;
        if (os.msync(self.ptr, self.size, os.MS_SYNC) < 0) {
            return error.SyncFailed;
        }
    }

    /// Clean up memory-mapped file
    pub fn deinit(self: *Self) void {
        os.munmap(self.ptr, self.size);
        if (self.fd >= 0) {
            _ = os.close(self.fd);
        }
        self.allocator.destroy(self);
    }
};

// ==================== MEMTABLE IMPLEMENTATION ====================

/// In-memory write buffer for LSM-Tree
pub const MemTable = struct {
    entries: std.ArrayList(KVEntry),
    max_size: usize,
    current_size: usize,
    heat_map: std.AutoHashMap(u128, f32),
    auto_flush_threshold: f32 = 0.8,
    allocator: Allocator,
    table_type: bdb_format.TableType,

    const Self = @This();

    /// Create a new MemTable
    pub fn init(allocator: Allocator, max_size: usize, table_type: bdb_format.TableType) !*Self {
        const memtable = try allocator.create(Self);
        
        memtable.* = .{
            .entries = std.ArrayList(KVEntry).init(allocator),
            .max_size = max_size,
            .current_size = 0,
            .heat_map = std.AutoHashMap(u128, f32).init(allocator),
            .allocator = allocator,
            .table_type = table_type,
        };

        return memtable;
    }

    /// Insert or update a key-value pair
    pub fn put(self: *Self, key: []const u8, value: []const u8, entry_type: EntryType) !void {
        // Update heat map
        const key_hash = std.hash.Wyhash.hash(key);
        const current_heat = self.heat_map.get(key_hash) orelse 0.0;
        const new_heat = @min(1.0, current_heat + 0.1);
        try self.heat_map.put(key_hash, new_heat);

        // Create new entry
        const entry = KVEntry.init(key, value, entry_type);
        
        // Check if this key already exists and update it
        for (self.entries.items, 0..) |*existing_entry, i| {
            if (std.mem.eql(u8, existing_entry.key, key)) {
                self.current_size -= existing_entry.getSize();
                self.entries.items[i] = entry;
                self.current_size += entry.getSize();
                return;
            }
        }

        // Add new entry
        try self.entries.append(entry);
        self.current_size += entry.getSize();
    }

    /// Get a value by key
    pub fn get(self: *Self, key: []const u8) ?KVEntry {
        // Check heat map
        const key_hash = std.hash.Wyhash.hash(key);
        if (self.heat_map.contains(key_hash)) {
            const heat = self.heat_map.get(key_hash).?;
            if (heat > 0.1) {
                // Update heat
                const new_heat = @min(1.0, heat * 1.1);
                self.heat_map.put(key_hash, new_heat) catch {};
            }
        }

        // Search for key (latest entry wins)
        for (0..self.entries.items.len) |i| {
            const entry_idx = self.entries.items.len - 1 - i;
            const entry = &self.entries.items[entry_idx];
            if (std.mem.eql(u8, entry.key, key) and !entry.deleted) {
                return entry.*;
            }
        }
        return null;
    }

    /// Delete a key
    pub fn delete(self: *Self, key: []const u8) !void {
        try self.put(key, &{}, .Delete);
    }

    /// Check if MemTable should be flushed
    pub fn shouldFlush(self: *const Self) bool {
        const usage_ratio = @as(f32, @floatFromInt(self.current_size)) / @as(f32, @floatFromInt(self.max_size));
        return usage_ratio >= self.auto_flush_threshold;
    }

    /// Get all entries as a sorted slice
    pub fn getAllEntries(self: *Self) ![]const KVEntry {
        // Sort entries by key for SSTable creation
        std.mem.sort(KVEntry, self.entries.items, {}, struct {
            fn less(_: void, a: KVEntry, b: KVEntry) bool {
                return std.mem.order(u8, a.key, b.key) == .lt;
            }
        });
        return self.entries.items;
    }

    /// Get current size
    pub fn getSize(self: *const Self) usize {
        return self.current_size;
    }

    /// Get entry count
    pub fn getCount(self: *const Self) usize {
        return self.entries.items.len;
    }

    /// Clear all entries
    pub fn clear(self: *Self) void {
        self.entries.clearRetainingCapacity();
        self.heat_map.clearRetainingCapacity();
        self.current_size = 0;
    }

    /// Clean up MemTable
    pub fn deinit(self: *Self) void {
        self.entries.deinit();
        self.heat_map.deinit();
        self.allocator.destroy(self);
    }
};

// ==================== SSTABLE LOADING STRUCTURES ====================

/// Information needed to load an SSTable from filename
pub const SSTableLoadInfo = struct {
    filename: []const u8,
    table_type: bdb_format.TableType,
    level: u8,
    timestamp: u64,
    entry_count: u64,
    
    const Self = @This();
    
    /// Sort by timestamp (newest first)
    pub fn less(a: Self, b: Self) bool {
        return a.timestamp > b.timestamp;
    }
};

// ==================== SSTABLE IMPLEMENTATION ====================

/// SSTable for on-disk storage with binary search indexing
pub const SSTable = struct {
    level: u8,
    file_path: []const u8,
    mmap_file: *MemoryMappedFile,
    index: std.ArrayList(IndexEntry),
    bloom_filter: ?*BloomFilter,
    creation_time: u64,
    table_type: bdb_format.TableType,
    allocator: Allocator,
    is_compacted: bool = false,

    const Self = @This();

    /// Create a new SSTable from MemTable data
    pub fn create(allocator: Allocator, level: u8, entries: []const KVEntry, base_path: []const u8, table_type: bdb_format.TableType) !*Self {
        const sstable = try allocator.create(Self);
        
        // Generate file path
        const timestamp = std.time.milliTimestamp();
        const file_name = std.fmt.allocPrintZ(allocator, "{s}_{}_{}_{}.sst", .{
            @tagName(table_type), level, timestamp, entries.len
        }) catch return error.OutOfMemory;
        defer allocator.free(file_name);
        
        const full_path = std.fs.path.joinZ(allocator, &.{ base_path, file_name }) catch return error.OutOfMemory;
        defer allocator.free(full_path);

        // Calculate required size
        var total_size: usize = 0;
        for (entries) |entry| {
            total_size += entry.getSize() + @sizeOf(IndexEntry);
        }
        total_size += 4096; // Bloom filter space

        // Create memory-mapped file
        const mmap_file = try MemoryMappedFile.create(allocator, full_path, total_size, false);

        // Write data and build index
        var offset: usize = 0;
        var index_entries = std.ArrayList(IndexEntry).init(allocator);

        for (entries) |entry| {
            if (entry.deleted) continue; // Skip deleted entries

            const index_entry = IndexEntry{
                .key = entry.key,
                .position = offset,
                .size = entry.getSize(),
                .timestamp = entry.timestamp,
            };
            try index_entries.append(index_entry);

            // Write entry to file
            try mmap_file.write(offset, std.mem.asBytes(&entry.entry_type));
            offset += @sizeOf(EntryType);
            
            try mmap_file.write(offset, std.mem.asBytes(&entry.timestamp));
            offset += @sizeOf(u64);
            
            try mmap_file.write(offset, std.mem.asBytes(&entry.key.len));
            offset += @sizeOf(usize);
            
            try mmap_file.write(offset, entry.key);
            offset += entry.key.len;
            
            try mmap_file.write(offset, std.mem.asBytes(&entry.value.len));
            offset += @sizeOf(usize);
            
            try mmap_file.write(offset, entry.value);
            offset += entry.value.len;
        }

        // Sort index for binary search
        std.mem.sort(IndexEntry, index_entries.items, {}, IndexEntry.less);

        sstable.* = .{
            .level = level,
            .file_path = full_path,
            .mmap_file = mmap_file,
            .index = index_entries,
            .bloom_filter = null,
            .creation_time = timestamp,
            .table_type = table_type,
            .allocator = allocator,
            .is_compacted = false,
        };

        // Sync to disk
        try mmap_file.sync();

        return sstable;
    }

    /// Load existing SSTable from file with complete metadata extraction
    pub fn load(allocator: Allocator, file_path: []const u8, fallback_table_type: bdb_format.TableType) !*Self {
        // Extract metadata from filename
        const filename = std.fs.path.basename(file_path);
        const load_info = parseSSTableFilenameFromPath(allocator, filename) catch {
            std.debug.print("⚠️ Failed to parse filename {s}, using fallback metadata\n", .{filename});
            // Return fallback metadata if filename parsing fails
            const file = try std.fs.openFileAbsolute(file_path, .{});
            defer file.close();
            const stat = try file.stat();
            const mmap_file = try MemoryMappedFile.create(allocator, file_path, stat.size, true);
            
            const sstable = try allocator.create(Self);
            sstable.* = .{
                .level = 0,
                .file_path = file_path,
                .mmap_file = mmap_file,
                .index = std.ArrayList(IndexEntry).init(allocator),
                .bloom_filter = null,
                .creation_time = std.time.milliTimestamp(),
                .table_type = fallback_table_type,
                .allocator = allocator,
                .is_compacted = false,
            };
            return sstable;
        };
        
        const detected_table_type = load_info.table_type;
        const detected_level = load_info.level;
        const creation_timestamp = load_info.timestamp;
        const expected_entry_count = load_info.entry_count;
        
        // Validate file integrity before processing
        const file = try std.fs.openFileAbsolute(file_path, .{});
        defer file.close();
        const stat = try file.stat();
        
        // Check minimum file size for validity
        const min_header_size = @sizeOf(EntryType) + @sizeOf(u64) + @sizeOf(usize) * 2;
        if (stat.size < min_header_size) {
            std.debug.print("⚠️ File {s} too small ({d} bytes), treating as corrupted\n", .{file_path, stat.size});
            return error.CorruptedFile;
        }
        
        // Create memory-mapped file
        const mmap_file = try MemoryMappedFile.create(allocator, file_path, stat.size, true);
        
        // Build comprehensive index from file header and content
        const index_entries = try buildIndexFromFileContent(allocator, &mmap_file, expected_entry_count);
        
        // Validate index completeness
        const actual_entry_count = index_entries.items.len;
        if (expected_entry_count > 0 and actual_entry_count < expected_entry_count / 2) {
            std.debug.print("⚠️ File {s} has significantly fewer entries than expected ({d} vs {d})\n", .{
                file_path, actual_entry_count, expected_entry_count
            });
            // Continue processing but log the discrepancy
        }
        
        // Create SSTable with extracted metadata
        const sstable = try allocator.create(Self);
        sstable.* = .{
            .level = detected_level,
            .file_path = file_path,
            .mmap_file = mmap_file,
            .index = index_entries,
            .bloom_filter = null,
            .creation_time = creation_timestamp,
            .table_type = detected_table_type,
            .allocator = allocator,
            .is_compacted = false,
        };
        
        // Update file statistics and metrics
        try updateSSTableMetrics(sstable, stat.size, actual_entry_count);
        
        // Build bloom filter for optimal read performance
        sstable.bloom_filter = try BloomFilter.init(allocator, actual_entry_count, 0.01);
        try buildBloomFilterFromIndex(sstable);
        
        return sstable;
    }

    /// Binary search for a key in SSTable
    pub fn get(self: *Self, key: []const u8) ?KVEntry {
        var left: usize = 0;
        var right: usize = self.index.items.len;

        while (left < right) {
            const mid = (left + right) / 2;
            const compare = std.mem.order(u8, key, self.index.items[mid].key);
            
            if (compare == .eq) {
                return self.readEntryAt(self.index.items[mid].position, self.index.items[mid].size);
            } else if (compare == .lt) {
                right = mid;
            } else {
                left = mid + 1;
            }
        }
        return null;
    }

    /// Read entry at specific position
    fn readEntryAt(self: *Self, position: usize, size: usize) ?KVEntry {
        const data = self.mmap_file.read(position, size) catch return null;
        if (data.len < size) return null;

        var offset: usize = 0;
        
        const entry_type = std.mem.bytesToValue(EntryType, data[offset..offset + @sizeOf(EntryType)]);
        offset += @sizeOf(EntryType);
        
        const timestamp = std.mem.bytesToValue(u64, data[offset..offset + @sizeOf(u64)]);
        offset += @sizeOf(u64);
        
        const key_len = std.mem.bytesToValue(usize, data[offset..offset + @sizeOf(usize)]);
        offset += @sizeOf(usize);
        
        if (offset + key_len + @sizeOf(usize) > data.len) return null;
        const key = data[offset..offset + key_len];
        offset += key_len;
        
        const value_len = std.mem.bytesToValue(usize, data[offset..offset + @sizeOf(usize)]);
        offset += @sizeOf(usize);
        
        if (offset + value_len > data.len) return null;
        const value = data[offset..offset + value_len];
        
        return KVEntry{
            .key = key,
            .value = value,
            .timestamp = timestamp,
            .entry_type = entry_type,
            .deleted = entry_type == .Delete,
        };
    }

    /// Get range of entries with optimized scanning
    pub fn getRange(self: *Self, start_key: []const u8, end_key: []const u8) ![]const KVEntry {
        if (!self.is_initialized) return error.NotInitialized;
        
        std.debug.print("🔍 Executing range query: {} to {}\n", .{ start_key, end_key });
        
        // Collect entries from all levels (newest first for correct timestamp priority)
        var all_entries = std.ArrayList(KVEntry).init(self.allocator);
        defer all_entries.deinit();
        
        // Search in MemTable first (most recent data)
        try self.searchMemTableRange(start_key, end_key, &all_entries);
        
        // Search in SSTables across all levels
        for (0..10) |level_num| {
            const level = &self.levels[level_num];
            for (level.sstables.items) |sstable| {
                try self.searchSSTableRange(sstable, start_key, end_key, &all_entries);
            }
        }
        
        // Sort by key, then by timestamp (newest first for same key)
        std.mem.sort(KVEntry, all_entries.items, {}, struct {
            fn less(a: KVEntry, b: KVEntry) bool {
                const key_cmp = std.mem.compare(u8, a.key, b.key);
                if (key_cmp != .eq) return key_cmp == .lt;
                return a.timestamp > b.timestamp; // Newest first
            }
        });
        
        // Deduplicate entries (keep latest timestamp per key)
        const deduplicated = try self.deduplicateRangeResults(all_entries.items);
        
        std.debug.print("📊 Range query returned {} unique entries\n", .{deduplicated.len});
        return deduplicated;
    }
    
    /// Search MemTable for range entries
    fn searchMemTableRange(self: *Self, start_key: []const u8, end_key: []const u8, results: *std.ArrayList(KVEntry)) !void {
        for (self.memtable.entries.items) |*entry| {
            if (entry.deleted) continue;
            
            // Check if key is within range
            const start_cmp = std.mem.compare(u8, entry.key, start_key);
            const end_cmp = std.mem.compare(u8, entry.key, end_key);
            
            if (start_cmp >= 0 and end_cmp <= 0) {
                try results.append(entry.*);
            }
        }
    }
    
    /// Search SSTable for range entries
    fn searchSSTableRange(self: *Self, sstable: *SSTable, start_key: []const u8, end_key: []const u8, results: *std.ArrayList(KVEntry)) !void {
        // Use binary search to find start position in index
        const start_idx = self.findLowerBoundInIndex(&sstable.index, start_key);
        if (start_idx == null) return; // No entries in range
        
        // Scan forward until we exceed end_key
        var idx = start_idx.?;
        while (idx < sstable.index.items.len) {
            const index_entry = sstable.index.items[idx];
            const cmp = std.mem.compare(u8, index_entry.key, end_key);
            
            if (cmp > 0) break; // Exceeded range
            
            // Read entry at this position
            const entry_data = self.readEntryAtPosition(sstable, index_entry) catch continue;
            
            if (entry_data) |entry| {
                try results.append(entry);
            }
            
            idx += 1;
        }
    }
    
    /// Find lower bound (first entry >= key) in sorted index
    fn findLowerBoundInIndex(self: *Self, index: *std.ArrayList(IndexEntry), key: []const u8) ?usize {
        var left: usize = 0;
        var right = index.items.len;
        
        while (left < right) {
            const mid = (left + right) / 2;
            const cmp = std.mem.compare(u8, index.items[mid].key, key);
            
            if (cmp >= 0) {
                right = mid;
            } else {
                left = mid + 1;
            }
        }
        
        return if (left < index.items.len) left else null;
    }
    
    /// Deduplicate range query results (keep latest timestamp per key)
    fn deduplicateRangeResults(self: *Self, entries: []KVEntry) ![]const KVEntry {
        var deduplicated = std.ArrayList(KVEntry).init(self.allocator);
        
        var i: usize = 0;
        while (i < entries.len) {
            const current_key = entries[i].key;
            var latest_entry = entries[i];
            
            // Find all entries with same key
            var j = i + 1;
            while (j < entries.len and std.mem.eql(u8, entries[j].key, current_key)) {
                // Keep the entry with the latest timestamp
                if (entries[j].timestamp > latest_entry.timestamp) {
                    latest_entry = entries[j];
                }
                j += 1;
            }
            
            // Add the latest entry
            try deduplicated.append(latest_entry);
            i = j;
        }
        
        return deduplicated.toOwnedSlice() catch return error.OutOfMemory;
    }
    
    /// Get prefix range entries (keys starting with prefix)
    pub fn getPrefixRange(self: *Self, prefix: []const u8) ![]const KVEntry {
        // Find end of prefix range
        var end_prefix = std.ArrayList(u8).init(self.allocator);
        defer end_prefix.deinit();
        
        // Create end key by incrementing last character
        try end_prefix.appendSlice(prefix);
        if (end_prefix.items.len > 0) {
            end_prefix.items[end_prefix.items.len - 1] += 1;
        }
        
        // Add null byte to ensure we get all keys with this prefix
        try end_prefix.append(0);
        
        const end_key = end_prefix.toOwnedSlice() catch return error.OutOfMemory;
        defer self.allocator.free(end_key);
        
        return self.getRange(prefix, end_key);
    }
    
    /// Get entries with pagination support
    pub fn getRangePaginated(self: *Self, start_key: []const u8, end_key: []const u8, offset: usize, limit: usize) !struct {
        entries: []const KVEntry,
        has_more: bool,
        total_count: usize,
    } {
        const all_entries = try self.getRange(start_key, end_key);
        const total_count = all_entries.len;
        
        const start_idx = @min(offset, total_count);
        const end_idx = @min(start_idx + limit, total_count);
        
        const page_entries = all_entries[start_idx..end_idx];
        const has_more = end_idx < total_count;
        
        return .{
            .entries = page_entries,
            .has_more = has_more,
            .total_count = total_count,
        };
    }
    
    /// Hot range query with heat tracking optimization
    pub fn getHotRange(self: *Self, start_key: []const u8, end_key: []const u8, min_heat: f32) ![]const KVEntry {
        var results = std.ArrayList(KVEntry).init(self.allocator);
        
        // Get all entries in range
        const range_entries = try self.getRange(start_key, end_key);
        
        // Filter by heat threshold
        for (range_entries) |entry| {
            const hash = std.hash.Wyhash.hash(entry.key);
            const heat = self.memtable.heat_map.get(hash) orelse 0.5;
            
            if (heat >= min_heat) {
                try results.append(entry);
            }
        }
        
        return results.toOwnedSlice() catch return error.OutOfMemory;
    }

    /// Get file size
    pub fn getSize(self: *const Self) usize {
        return self.mmap_file.size;
    }

    /// Clean up SSTable
    pub fn deinit(self: *Self) void {
        self.mmap_file.deinit();
        self.index.deinit();
        if (self.bloom_filter) |bf| {
            bf.deinit();
        }
        self.allocator.destroy(self);
    }
};

// ==================== BLOOM FILTER ====================

/// Bloom filter for fast negative lookups
pub const BloomFilter = struct {
    bit_array: std.ArrayList(u8),
    num_bits: usize,
    num_hashes: usize,
    num_elements: usize,
    allocator: Allocator,

    const Self = @This();

    pub fn init(allocator: Allocator, expected_elements: usize, false_positive_rate: f32) !*Self {
        const bit_array = try allocator.create(Self);
        
        // Calculate optimal parameters
        const num_bits = @as(usize, @intFromFloat(-(@as(f32, @floatFromInt(expected_elements)) * 
                                    @log(@as(f32, @floatFromInt(2))) / 
                                    @log(1.0 - false_positive_rate))));
        const num_hashes = @as(usize, @intFromFloat((@as(f32, @floatFromInt(num_bits)) / 
                                    @as(f32, @floatFromInt(expected_elements))) * @log(2.0)));

        const num_bytes = (num_bits + 7) / 8;
        var bits = std.ArrayList(u8).init(allocator);
        try bits.appendNTimes(0, num_bytes);

        bit_array.* = .{
            .bit_array = bits,
            .num_bits = num_bits,
            .num_hashes = num_hashes,
            .num_elements = 0,
            .allocator = allocator,
        };

        return bit_array;
    }

    pub fn add(self: *Self, key: []const u8) void {
        for (0..self.num_hashes) |i| {
            const hash_value = self.hashFunction(key, i);
            const bit_index = @as(usize, @intCast(hash_value % @as(u64, @intCast(self.num_bits))));
            const byte_index = bit_index / 8;
            const bit_offset = bit_index % 8;
            self.bit_array.items[byte_index] |= @as(u8, 1) << @as(u3, @intCast(bit_offset));
        }
        self.num_elements += 1;
    }

    pub fn mightContain(self: *const Self, key: []const u8) bool {
        for (0..self.num_hashes) |i| {
            const hash_value = self.hashFunction(key, i);
            const bit_index = @as(usize, @intCast(hash_value % @as(u64, @intCast(self.num_bits))));
            const byte_index = bit_index / 8;
            const bit_offset = bit_index % 8;
            if (self.bit_array.items[byte_index] & (@as(u8, 1) << @as(u3, @intCast(bit_offset))) == 0) {
                return false;
            }
        }
        return true;
    }

    fn hashFunction(self: *const Self, key: []const u8, seed: usize) u64 {
        return std.hash.Wyhash.hashWithSeed(key, seed);
    }

    pub fn getFalsePositiveRate(self: *const Self) f32 {
        return @as(f32, @floatFromInt(1)) - std.math.pow(f32, -@as(f32, @floatFromInt(self.num_hashes)) * 
                                                       @as(f32, @floatFromInt(self.num_elements)) / 
                                                       @as(f32, @floatFromInt(self.num_bits)));
    }
};

// ==================== COMPACTION ENGINE ====================

/// Compaction strategies for LSM-Tree optimization
pub const CompactionStrategy = enum {
    /// Level-based compaction (balanced)
    Leveled,      
    /// Size-tiered compaction (write-optimized)  
    SizeTiered,   
    /// Adaptive strategy selection
    Hybrid,       
};

/// Compaction manager for background optimization
pub const CompactionManager = struct {
    levels: [10]Level,
    strategy: CompactionStrategy,
    allocator: Allocator,
    max_concurrent: usize = 4,
    active_compactions: usize = 0,

    const Self = @This();

    pub fn init(allocator: Allocator, strategy: CompactionStrategy) !*Self {
        const manager = try allocator.create(Self);
        
        // Initialize all levels with default configuration
        for (0..10) |i| {
            manager.levels[i] = Level{
                .level_num = @as(u8, @intCast(i)),
                .max_size = @as(usize, 1) << @as(u6, @intCast(20 + i * 2)), // Exponential growth
                .sstables = std.ArrayList(*SSTable).init(allocator),
                .target_files = 10,
            };
        }

        manager.* = .{
            .levels = manager.levels,
            .strategy = strategy,
            .allocator = allocator,
        };

        return manager;
    }

    /// Schedule compaction for a specific level
    pub fn schedule(self: *Self, level_num: u8) !void {
        if (level_num >= 10) return error.InvalidLevel;
        if (self.active_compactions >= self.max_concurrent) return error.TooManyCompactions;

        const level = &self.levels[level_num];
        if (level.sstables.items.len > level.target_files * 2) {
            try self.execute(level_num);
        }
    }

    /// Execute compaction for a specific level
    pub fn execute(self: *Self, level_num: u8) !void {
        if (level_num >= 10) return error.InvalidLevel;
        if (self.active_compactions >= self.max_concurrent) return error.TooManyCompactions;

        self.active_compactions += 1;
        defer self.active_compactions -= 1;

        const level = &self.levels[level_num];
        if (level.sstables.items.len <= 1) return;

        std.debug.print("🔧 Starting compaction for level {} ({} SSTables)\n", .{
            level_num, level.sstables.items.len
        });

        // Sort SSTables by creation time
        std.mem.sort(*SSTable, level.sstables.items, {}, struct {
            fn less(_: void, a: *SSTable, b: *SSTable) bool {
                return a.creation_time < b.creation_time;
            }
        });

        // Merge oldest SSTables
        const merge_count = std.math.min(level.sstables.items.len / 2, 10);
        const to_merge = level.sstables.items[0..merge_count];
        
        // Implement actual merging logic
        std.debug.print("🔄 Merging {} SSTables into level {}\n", .{ merge_count, level_num });
        
        try self.performMerge(level_num, to_merge);
    }
    
    /// Perform the actual merge operation
    fn performMerge(self: *Self, level_num: u8, sstables_to_merge: []*SSTable) !void {
        const level = &self.levels[level_num];
        
        // Step 1: Read all entries from SSTables to merge
        const all_entries = try self.readAllEntriesFromSSTables(sstables_to_merge);
        defer self.allocator.free(all_entries);
        
        if (all_entries.len == 0) {
            std.debug.print("⚠️ No entries found in SSTables to merge\n", .{});
            return;
        }
        
        std.debug.print("📊 Read {} total entries for merging\n", .{all_entries.len});
        
        // Step 2: Sort and deduplicate entries (keep latest timestamp)
        const merged_entries = try self.mergeAndDedupEntries(all_entries);
        defer self.allocator.free(merged_entries);
        
        std.debug.print("🔄 Merged to {} unique entries (after deduplication)\n", .{merged_entries.len});
        
        // Step 3: Create new merged SSTable
        const new_sstable = try SSTable.create(
            self.allocator, 
            level_num, 
            merged_entries, 
            self.base_path,
            sstables_to_merge[0].table_type
        );
        
        // Step 4: Replace old SSTables with new one
        try self.replaceSSTables(level_num, sstables_to_merge, new_sstable);
        
        // Step 5: Trigger next level compaction if needed
        if (level_num + 1 < 10) {
            const next_level = &self.levels[level_num + 1];
            if (next_level.sstables.items.len >= next_level.target_files) {
                try self.schedule(level_num + 1);
            }
        }
        
        std.debug.print("✅ Compaction completed for level {}\n", .{level_num});
    }
    
    /// Read all entries from multiple SSTables
    fn readAllEntriesFromSSTables(self: *Self, sstables: []*SSTable) ![]KVEntry {
        var all_entries = std.ArrayList(KVEntry).init(self.allocator);
        
        for (sstables) |sstable| {
            // Read all entries from this SSTable's file
            const entries = try self.readEntriesFromFile(sstable);
            defer self.allocator.free(entries);
            
            // Add entries to our collection
            for (entries) |entry| {
                try all_entries.append(entry);
            }
        }
        
        return all_entries.toOwnedSlice() catch return error.OutOfMemory;
    }
    
    /// Read entries from a single SSTable file
    fn readEntriesFromFile(self: *Self, sstable: *SSTable) ![]KVEntry {
        var entries = std.ArrayList(KVEntry).init(self.allocator);
        
        // Read all entries using the file's index
        for (sstable.index.items) |index_entry| {
            // Read entry data from file
            const entry_data = self.readEntryAtPosition(sstable, index_entry) catch {
                continue; // Skip corrupted entries
            };
            
            if (entry_data) |entry| {
                try entries.append(entry);
            }
        }
        
        return entries.toOwnedSlice() catch return error.OutOfMemory;
    }
    
/// Read entry data at specific position
    fn readEntryAtPosition(self: *Self, sstable: *SSTable, index_entry: IndexEntry) !?KVEntry {
        // Read entry type
        var entry_type_bytes: [@sizeOf(EntryType)]u8 = undefined;
        sstable.mmap_file.read(index_entry.position, &entry_type_bytes) catch return null;
        const entry_type = @as(EntryType, @enumFromInt(entry_type_bytes[0]));
        
        // Skip deleted entries
        if (entry_type == .Delete) return null;
        
        // Calculate position after entry type
        var offset = index_entry.position + @sizeOf(EntryType);
        
        // Read timestamp (8 bytes, little endian)
        var timestamp_bytes: [@sizeOf(u64)]u8 = undefined;
        sstable.mmap_file.read(offset, &timestamp_bytes) catch return null;
        const timestamp = std.mem.readIntSlice(u64, &timestamp_bytes, .little);
        offset += @sizeOf(u64);
        
        // Read key length (usize bytes, little endian)
        var key_len_bytes: [@sizeOf(usize)]u8 = undefined;
        sstable.mmap_file.read(offset, &key_len_bytes) catch return null;
        const key_len = std.mem.readIntSlice(usize, &key_len_bytes, .little);
        offset += @sizeOf(usize);
        
        // Read key
        const key_data = sstable.mmap_file.read(offset, key_len) catch return null;
        offset += key_len;
        
        // Read value length
        var value_len_bytes: [@sizeOf(usize)]u8 = undefined;
        sstable.mmap_file.read(offset, &value_len_bytes) catch return null;
        const value_len = std.mem.readIntSlice(usize, &value_len_bytes, .little);
        offset += @sizeOf(usize);
        
        // Read value
        const value_data = sstable.mmap_file.read(offset, value_len) catch return null;
        
        // Create KVEntry with proper memory allocation
        const key_copy = try self.allocator.alloc(u8, key_len);
        @memcpy(key_copy, key_data);
        
        const value_copy = try self.allocator.alloc(u8, value_len);
        @memcpy(value_copy, value_data);
        
        const kv_entry = KVEntry{
            .key = key_copy,
            .value = value_copy,
            .entry_type = if (entry_type == .Update) .Update else .Insert,
            .timestamp = timestamp,
            .deleted = false,
            .heat = 0.5,
        };
        
        return kv_entry;
    }
    
    /// Merge and deduplicate entries (keep latest timestamp per key)
    fn mergeAndDedupEntries(self: *Self, entries: []KVEntry) ![]KVEntry {
        // Sort entries by key, then by timestamp (newest first)
        std.mem.sort(KVEntry, entries, {}, struct {
            fn less(a: KVEntry, b: KVEntry) bool {
                const key_cmp = std.mem.compare(u8, a.key, b.key);
                if (key_cmp != .eq) return key_cmp == .lt;
                return a.timestamp > b.timestamp; // Newest first
            }
        });
        
        var merged = std.ArrayList(KVEntry).init(self.allocator);
        
        var i: usize = 0;
        while (i < entries.len) {
            const current_key = entries[i].key;
            var latest_entry = entries[i];
            
            // Find all entries with same key
            var j = i + 1;
            while (j < entries.len and std.mem.eql(u8, entries[j].key, current_key)) {
                // Keep the entry with the latest timestamp
                if (entries[j].timestamp > latest_entry.timestamp) {
                    latest_entry = entries[j];
                }
                j += 1;
            }
            
            // Add the latest entry
            try merged.append(latest_entry);
            i = j;
        }
        
        return merged.toOwnedSlice() catch return error.OutOfMemory;
    }
    
    /// Replace old SSTables with new merged SSTable
    fn replaceSSTables(self: *Self, level_num: u8, old_sstables: []*SSTable, new_sstable: *SSTable) !void {
        const level = &self.levels[level_num];
        
        // Add new SSTable to level
        try level.sstables.append(new_sstable);
        
        // Remove and clean up old SSTables
        for (old_sstables) |old_sstable| {
            // Remove from level
            for (level.sstables.items, 0..) |*sstable, idx| {
                if (sstable.* == old_sstable) {
                    _ = level.sstables.orderedRemove(idx);
                    break;
                }
            }
            
            // Clean up old SSTable
            self.cleanupSSTable(old_sstable);
        }
        
        std.debug.print("🗑️ Cleaned up {} old SSTables\n", .{old_sstables.len});
    }
    
    /// Clean up old SSTable and remove files
    fn cleanupSSTable(self: *Self, sstable: *SSTable) void {
        // Close memory-mapped file
        sstable.mmap_file.deinit();
        
        // Remove file from disk
        std.fs.cwd().deleteFile(sstable.file_path) catch {
            std.debug.print("⚠️ Failed to delete SSTable file: {s}\n", .{sstable.file_path});
        };
        
        // Free memory
        self.allocator.free(sstable.file_path);
        self.allocator.destroy(sstable);
    }
    
    /// Clean up old SSTables after compaction
    fn cleanupLevel(self: *Self, level_num: u8) !void {
        const level = &self.levels[level_num];
        
        // Clean up any orphaned SSTables
        // This is handled by replaceSSTables now
        std.debug.print("🧹 Level {} cleanup completed\n", .{level_num});
    }

    /// Get compaction statistics
    pub fn getStats(self: *const Self) CompactionStats {
        var total_sstables: usize = 0;
        var total_size: usize = 0;
        
        for (0..10) |i| {
            total_sstables += self.levels[i].sstables.items.len;
            for (self.levels[i].sstables.items) |sstable| {
                total_size += sstable.getSize();
            }
        }

        return CompactionStats{
            .total_levels = 10,
            .total_sstables = total_sstables,
            .total_size = total_size,
            .active_compactions = self.active_compactions,
        };
    }

    /// Clean up compaction manager
    pub fn deinit(self: *Self) void {
        for (0..10) |i| {
            self.levels[i].sstables.deinit();
        }
        self.allocator.destroy(self);
    }
};

/// Storage level configuration and state
pub const Level = struct {
    level_num: u8,
    max_size: usize,
    sstables: std.ArrayList(*SSTable),
    target_files: usize,
};

/// Compaction statistics
pub const CompactionStats = struct {
    total_levels: usize,
    total_sstables: usize,
    total_size: usize,
    active_compactions: usize,
};

// ==================== MULTI-LEVEL STORAGE ORGANIZATION ====================

/// Multi-level storage for optimal read/write performance
pub const LSMTree = struct {
    memtable: *MemTable,
    compaction_manager: *CompactionManager,
    levels: [10]Level,
    allocator: Allocator,
    base_path: []const u8,
    table_type: bdb_format.TableType,
    is_initialized: bool,

    const Self = @This();

    /// Initialize LSM-Tree storage engine
    pub fn init(allocator: Allocator, base_path: []const u8, table_type: bdb_format.TableType, max_memtable_size: usize) !*Self {
        const lsm_tree = try allocator.create(Self);
        
        // Initialize MemTable
        const memtable = try MemTable.init(allocator, max_memtable_size, table_type);

        // Initialize compaction manager
        const compaction_manager = try CompactionManager.init(allocator, .Leveled);

        // Initialize levels
        var levels: [10]Level = undefined;
        for (0..10) |i| {
            levels[i] = Level{
                .level_num = @as(u8, @intCast(i)),
                .max_size = @as(usize, 1) << @as(u6, @intCast(20 + i * 2)),
                .sstables = std.ArrayList(*SSTable).init(allocator),
                .target_files = if (i == 0) 4 else 10,
            };
        }

        lsm_tree.* = .{
            .memtable = memtable,
            .compaction_manager = compaction_manager,
            .levels = levels,
            .allocator = allocator,
            .base_path = base_path,
            .table_type = table_type,
            .is_initialized = true,
        };

        return lsm_tree;
    }

    /// Put key-value pair
    pub fn put(self: *Self, key: []const u8, value: []const u8) !void {
        if (!self.is_initialized) return error.NotInitialized;
        
        try self.memtable.put(key, value, .Insert);
        
        // Check if we need to flush
        if (self.memtable.shouldFlush()) {
            try self.flush();
        }
    }

    /// Get value by key
    pub fn get(self: *Self, key: []const u8) ?KVEntry {
        if (!self.is_initialized) return null;
        
        // Check MemTable first
        if (self.memtable.get(key)) |entry| {
            return entry;
        }
        
        // Check SSTables (latest level first)
        for (0..10) |i| {
            const level_idx = 9 - i; // Check highest levels first
            for (self.levels[level_idx].sstables.items) |sstable| {
                if (sstable.get(key)) |entry| {
                    return entry;
                }
            }
        }
        
        return null;
    }

    /// Delete key
    pub fn delete(self: *Self, key: []const u8) !void {
        if (!self.is_initialized) return error.NotInitialized;
        try self.memtable.delete(key);
        
        if (self.memtable.shouldFlush()) {
            try self.flush();
        }
    }

    /// Flush MemTable to SSTable
    pub fn flush(self: *Self) !void {
        if (!self.is_initialized) return error.NotInitialized;
        if (self.memtable.getCount() == 0) return;

        // Get all entries from MemTable
        const entries = try self.memtable.getAllEntries();
        
        // Create new SSTable
        const sstable = try SSTable.create(self.allocator, 0, entries, self.base_path, self.table_type);
        
        // Add to level 0
        try self.levels[0].sstables.append(sstable);
        
        // Clear MemTable
        self.memtable.clear();
        
        // Schedule compaction
        try self.compaction_manager.schedule(0);
    }

    /// Compact all levels
    pub fn compact(self: *Self) !void {
        for (0..10) |i| {
            try self.compaction_manager.schedule(@as(u8, @intCast(i)));
        }
    }

    /// Get storage statistics
    pub fn getStats(self: *const Self) !LSMTreeStats {
        if (!self.is_initialized) return error.NotInitialized;

        const memtable_size = self.memtable.getSize();
        const memtable_count = self.memtable.getCount();
        
        var total_sstables: usize = 0;
        var total_sstable_size: usize = 0;
        var total_entries: usize = 0;
        
        for (0..10) |i| {
            total_sstables += self.levels[i].sstables.items.len;
            for (self.levels[i].sstables.items) |sstable| {
                total_sstable_size += sstable.getSize();
            }
        }

        const compaction_stats = self.compaction_manager.getStats();

        return LSMTreeStats{
            .memtable_size = memtable_size,
            .memtable_count = memtable_count,
            .total_sstables = total_sstables,
            .total_sstable_size = total_sstable_size,
            .total_levels = 10,
            .active_compactions = compaction_stats.active_compactions,
        };
    }

    /// Load existing SSTables from disk
    pub fn loadFromDisk(self: *Self) !void {
        if (!self.is_initialized) return error.NotInitialized;
        
        std.debug.print("🔍 Loading existing SSTables from disk...\n", .{});
        
        // Open directory and scan for .sst files
        var dir = std.fs.cwd().openDir(self.base_path, .{}) catch {
            std.debug.print("📁 Directory does not exist, creating new: {s}\n", .{self.base_path});
            try std.fs.cwd().makePath(self.base_path);
            return;
        };
        defer dir.close();
        
        var loaded_count: usize = 0;
        var file_list = std.ArrayList(SSTableLoadInfo).init(self.allocator);
        defer file_list.deinit();
        
        // Scan directory for .sst files
        var iter = dir.iterate();
        while (try iter.next()) |entry| {
            if (entry.kind == .file and std.mem.endsWith(u8, entry.name, ".sst")) {
                const load_info = try self.parseSSTableFilename(entry.name);
                if (load_info) |info| {
                    try file_list.append(info);
                }
            }
        }
        
        // Sort files by timestamp (newest first)
        std.sort.sort(SSTableLoadInfo, file_list.items, {}, SSTableLoadInfo.less);
        
        // Load each SSTable
        for (file_list.items) |sstable_info| {
            const sstable = self.loadSSTableFile(sstable_info) catch |err| {
                std.debug.print("⚠️ Failed to load SSTable {s}: {}\n", .{ sstable_info.filename, err });
                continue; // Skip corrupted files and continue loading
            };
            
            if (sstable_info.level < 10) {
                try self.levels[sstable_info.level].sstables.append(sstable);
                loaded_count += 1;
                std.debug.print("✅ Loaded SSTable {s} to level {d}\n", .{ sstable_info.filename, sstable_info.level });
            } else {
                std.debug.print("⚠️ Invalid level {d} for {s}, skipping\n", .{ sstable_info.level, sstable_info.filename });
                sstable.deinit();
            }
        }
        
        // Build bloom filters for loaded SSTables
        for (0..10) |level| {
            for (self.levels[level].sstables.items) |sstable| {
                if (sstable.bloom_filter == null) {
                    sstable.bloom_filter = try BloomFilter.create(self.allocator, 1000, 0.01);
                    try sstable.buildBloomFilter();
                }
            }
        }
        
        std.debug.print("🎉 Successfully loaded {} SSTables from disk\n", .{loaded_count});
    }
    
    /// Parse SSTable filename to extract metadata
    fn parseSSTableFilename(self: *Self, filename: []const u8) !?SSTableLoadInfo {
        // Expected format: {table_type}_{level}_{timestamp}_{entry_count}.sst
        // Example: history_1_1672531200000_1500.sst
        
        const dot_idx = std.mem.lastIndexOf(u8, filename, ".") orelse return null;
        if (!std.mem.eql(u8, filename[dot_idx..], ".sst")) return null;
        
        const base_name = filename[0..dot_idx];
        var parts = std.mem.split(u8, base_name, "_");
        
        // Parse table type
        const table_type_str = parts.next() orelse return null;
        const table_type = std.meta.stringToEnum(bdb_format.TableType, table_type_str) orelse return null;
        
        // Parse level
        const level_str = parts.next() orelse return null;
        const level = std.fmt.parseInt(u8, level_str, 10) catch return null;
        if (level >= 10) return null; // Invalid level
        
        // Parse timestamp
        const timestamp_str = parts.next() orelse return null;
        const timestamp = std.fmt.parseInt(u64, timestamp_str, 10) catch return null;
        
        // Parse entry count
        const entry_count_str = parts.next() orelse return null;
        const entry_count = std.fmt.parseInt(u64, entry_count_str, 10) catch return null;
        
        return SSTableLoadInfo{
            .filename = try self.allocator.dupe(u8, filename),
            .table_type = table_type,
            .level = level,
            .timestamp = timestamp,
            .entry_count = entry_count,
        };
    }
    
    /// Load a single SSTable file
    fn loadSSTableFile(self: *Self, info: SSTableLoadInfo) !*SSTable {
        const full_path = try std.fmt.allocPrint(self.allocator, "{s}/{s}", .{
            self.base_path, info.filename
        });
        defer self.allocator.free(full_path);
        
        // Get file statistics
        const stat = try std.fs.cwd().statFile(full_path);
        
        // Create memory-mapped file
        const mmap_file = try MemoryMappedFile.create(self.allocator, full_path, stat.size, true);
        
        // Load SSTable structure
        const sstable = try self.allocator.create(SSTable);
        sstable.* = SSTable{
            .level = info.level,
            .file_path = try self.allocator.dupe(u8, full_path),
            .mmap_file = mmap_file,
            .index = std.ArrayList(IndexEntry).init(self.allocator),
            .bloom_filter = null,
            .creation_time = info.timestamp,
            .table_type = info.table_type,
            .allocator = self.allocator,
            .is_compacted = false,
        };
        
        // Build index from loaded file
        try sstable.buildIndexFromFile();
        
        return sstable;
    }
    
    /// Build index from SSTable file content
    fn buildIndexFromFile(sstable: *SSTable) !void {
        const file_size = sstable.mmap_file.size;
        var offset: usize = 0;
        
        while (offset < file_size) {
            // Parse entry at current offset
            const entry_info = try sstable.parseEntryAt(offset);
            if (entry_info == null) break; // Reached end of file or error
            
            const info = entry_info.?;
            
            // Add to index
            try sstable.index.append(IndexEntry{
                .key = info.key,
                .position = offset,
                .size = info.size,
                .timestamp = info.timestamp,
            });
            
            // Move to next entry
            offset += info.size;
            
            // Safety check to prevent infinite loops
            if (offset <= sstable.mmap_file.size and sstable.index.items.len > 1000000) {
                break; // Safety limit
            }
        }
        
        // Sort index for binary search
        std.mem.sort(IndexEntry, sstable.index.items, {}, IndexEntry.less);
    }
    
    /// Parse a single entry at given offset
    fn parseEntryAt(sstable: *SSTable, offset: usize) !?struct {
        key: []const u8,
        timestamp: u64,
        size: usize,
    } {
        if (offset + @sizeOf(EntryType) + @sizeOf(u64) + @sizeOf(usize) * 2 >= sstable.mmap_file.size) {
            return null; // Not enough data for a complete entry
        }
        
        // Read entry type
        var entry_type_bytes: [@sizeOf(EntryType)]u8 = undefined;
        try sstable.mmap_file.read(offset, &entry_type_bytes);
        const entry_type = @as(EntryType, @enumFromInt(entry_type_bytes[0]));
        offset += @sizeOf(EntryType);
        
        // Read timestamp (8 bytes, little endian)
        var timestamp_bytes: [@sizeOf(u64)]u8 = undefined;
        try sstable.mmap_file.read(offset, &timestamp_bytes);
        const timestamp = std.mem.readIntSlice(u64, &timestamp_bytes, .little);
        offset += @sizeOf(u64);
        
        // Read key length (usize bytes, little endian)
        var key_len_bytes: [@sizeOf(usize)]u8 = undefined;
        try sstable.mmap_file.read(offset, &key_len_bytes);
        const key_len = std.mem.readIntSlice(usize, &key_len_bytes, .little);
        offset += @sizeOf(usize);
        
        // Read value length
        var value_len_bytes: [@sizeOf(usize)]u8 = undefined;
        try sstable.mmap_file.read(offset, &value_len_bytes);
        const value_len = std.mem.readIntSlice(usize, &value_len_bytes, .little);
        offset += @sizeOf(usize);
        
        // Validate remaining data
        if (offset + key_len + value_len > sstable.mmap_file.size) {
            return null; // Corrupted entry
        }
        
        // Read key
        const key = sstable.mmap_file.read(offset, key_len) catch return null;
        offset += key_len;
        
        // Skip value (not needed for index)
        // offset += value_len;
        
        const total_size = offset + value_len; // Full entry size
        
        return .{
            .key = key,
            .timestamp = timestamp,
            .size = total_size,
        };
    }
    
    /// Build bloom filter for SSTable
    fn buildBloomFilter(sstable: *SSTable) !void {
        if (sstable.bloom_filter == null) return;
        
        const bloom = sstable.bloom_filter.?;
        
        // Add all keys to bloom filter
        for (sstable.index.items) |index_entry| {
            try bloom.add(index_entry.key);
        }
    }

    /// Clean up LSM-Tree
    pub fn deinit(self: *Self) void {
        if (!self.is_initialized) return;
        
        // Clean up MemTable
        self.memtable.deinit();
        
        // Clean up SSTables
        for (0..10) |i| {
            for (self.levels[i].sstables.items) |sstable| {
                sstable.deinit();
            }
            self.levels[i].sstables.deinit();
        }
        
        // Clean up compaction manager
        self.compaction_manager.deinit();
        
        self.allocator.destroy(self);
    }
};

/// LSM-Tree statistics
pub const LSMTreeStats = struct {
    memtable_size: usize,
    memtable_count: usize,
    total_sstables: usize,
    total_sstable_size: usize,
    total_levels: usize,
    active_compactions: usize,
};

// ==================== SSTABLE METADATA EXTRACTION HELPERS ====================

/// Parse SSTable filename to extract metadata (standalone version)
fn parseSSTableFilenameFromPath(allocator: Allocator, filename: []const u8) !SSTableLoadInfo {
    // Expected format: {table_type}_{level}_{timestamp}_{entry_count}.sst
    // Example: history_1_1672531200000_1500.sst
    
    const dot_idx = std.mem.lastIndexOf(u8, filename, ".") orelse return error.InvalidFilename;
    if (!std.mem.eql(u8, filename[dot_idx..], ".sst")) return error.InvalidFilename;
    
    const base_name = filename[0..dot_idx];
    var parts = std.mem.split(u8, base_name, "_");
    
    // Parse table type
    const table_type_str = parts.next() orelse return error.InvalidFilename;
    const table_type = std.meta.stringToEnum(bdb_format.TableType, table_type_str) orelse return error.InvalidTableType;
    
    // Parse level
    const level_str = parts.next() orelse return error.InvalidFilename;
    const level = std.fmt.parseInt(u8, level_str, 10) catch return error.InvalidLevel;
    if (level >= 10) return error.InvalidLevel;
    
    // Parse timestamp
    const timestamp_str = parts.next() orelse return error.InvalidFilename;
    const timestamp = std.fmt.parseInt(u64, timestamp_str, 10) catch return error.InvalidTimestamp;
    
    // Parse entry count
    const entry_count_str = parts.next() orelse return error.InvalidFilename;
    const entry_count = std.fmt.parseInt(u64, entry_count_str, 10) catch return error.InvalidEntryCount;
    
    return SSTableLoadInfo{
        .filename = try allocator.dupe(u8, filename),
        .table_type = table_type,
        .level = level,
        .timestamp = timestamp,
        .entry_count = entry_count,
    };
}

/// Build comprehensive index from file content with validation
fn buildIndexFromFileContent(allocator: Allocator, mmap_file: *MemoryMappedFile, expected_count: u64) !std.ArrayList(IndexEntry) {
    const index_entries = std.ArrayList(IndexEntry).init(allocator);
    const file_size = mmap_file.size;
    var offset: usize = 0;
    var entry_count: u64 = 0;
    var corrupted_entries: u64 = 0;
    
    std.debug.print("🔍 Building index from file ({} bytes, expecting {} entries)\n", .{ file_size, expected_count });
    
    while (offset < file_size) {
        // Parse entry at current offset with comprehensive validation
        const entry_info = parseEntryAtPosition(mmap_file, offset) catch {
            corrupted_entries += 1;
            if (corrupted_entries > 10) {
                std.debug.print("⚠️ Too many corrupted entries, stopping index building\\n", .{});
                break;
            }
            // Skip to next potential entry position
            offset += @sizeOf(EntryType);
            continue;
        };
        
        if (entry_info == null) {
            // Reached end of valid data
            break;
        }
        
        const info = entry_info.?;
        
        // Validate entry integrity
        if (info.size == 0 or info.key.len == 0) {
            corrupted_entries += 1;
            offset += @sizeOf(EntryType);
            continue;
        }
        
        // Add to index with proper memory management
        const key_copy = try allocator.alloc(u8, info.key.len);
        @memcpy(key_copy, info.key);
        
        try index_entries.append(IndexEntry{
            .key = key_copy,
            .position = offset,
            .size = info.size,
            .timestamp = info.timestamp,
        });
        
        entry_count += 1;
        offset += info.size;
        
        // Safety checks to prevent infinite loops
        if (offset <= file_size and entry_count > 1000000) {
            std.debug.print("⚠️ Reached safety limit of 1M entries\\n", .{});
            break;
        }
        
        // Check if we've exceeded expected count significantly
        if (expected_count > 0 and entry_count > expected_count * 2) {
            std.debug.print("⚠️ Entry count significantly exceeds expected, stopping\\n", .{});
            break;
        }
    }
    
    // Sort index for binary search
    std.mem.sort(IndexEntry, index_entries.items, {}, IndexEntry.less);
    
    std.debug.print("✅ Built index with {} entries ({} corrupted/skipped)\\n", .{ 
        index_entries.items.len, corrupted_entries 
    });
    
    return index_entries;
}

/// Parse entry at specific file position with enhanced error handling
fn parseEntryAtPosition(mmap_file: *MemoryMappedFile, offset: usize) !?struct {
    key: []const u8,
    timestamp: u64,
    size: usize,
} {
    // Check if we have enough space for minimum entry header
    const min_size = @sizeOf(EntryType) + @sizeOf(u64) + @sizeOf(usize) * 2;
    if (offset + min_size > mmap_file.size) {
        return null; // Not enough data for a complete entry
    }
    
    // Read entry type with bounds checking
    var entry_type_bytes: [@sizeOf(EntryType)]u8 = undefined;
    try mmap_file.read(offset, &entry_type_bytes);
    const entry_type = @as(EntryType, @enumFromInt(entry_type_bytes[0]));
    var current_offset = offset + @sizeOf(EntryType);
    
    // Read timestamp (8 bytes, little endian)
    var timestamp_bytes: [@sizeOf(u64)]u8 = undefined;
    try mmap_file.read(current_offset, &timestamp_bytes);
    const timestamp = std.mem.readIntSlice(u64, &timestamp_bytes, .little);
    current_offset += @sizeOf(u64);
    
    // Read key length (usize bytes, little endian)
    var key_len_bytes: [@sizeOf(usize)]u8 = undefined;
    try mmap_file.read(current_offset, &key_len_bytes);
    const key_len = std.mem.readIntSlice(usize, &key_len_bytes, .little);
    current_offset += @sizeOf(usize);
    
    // Read value length
    var value_len_bytes: [@sizeOf(usize)]u8 = undefined;
    try mmap_file.read(current_offset, &value_len_bytes);
    const value_len = std.mem.readIntSlice(usize, &value_len_bytes, .little);
    current_offset += @sizeOf(usize);
    
    // Validate lengths to prevent buffer overflow
    if (key_len > 1024 * 1024 or value_len > 1024 * 1024) { // 1MB limit per field
        return error.EntryTooLarge;
    }
    
    // Validate that we have enough data for the full entry
    if (current_offset + key_len + value_len > mmap_file.size) {
        return error.IncompleteEntry;
    }
    
    // Read key data
    const key_data = mmap_file.read(current_offset, key_len) catch return error.ReadFailed;
    current_offset += key_len;
    
    // Calculate total entry size
    const total_size = current_offset + value_len - offset;
    
    return .{
        .key = key_data,
        .timestamp = timestamp,
        .size = total_size,
    };
}

/// Update SSTable statistics and metrics
fn updateSSTableMetrics(sstable: *SSTable, file_size: usize, entry_count: usize) !void {
    // Update file-level statistics
    const avg_entry_size = if (entry_count > 0) @as(f32, @floatFromInt(file_size)) / @as(f32, @floatFromInt(entry_count)) else 0.0;
    
    std.debug.print("📊 SSTable metrics: {} bytes, {} entries, {:.2} bytes/entry\\n", .{
        file_size, entry_count, avg_entry_size
    });
    
    // Store metrics in bloom filter if available
    if (sstable.bloom_filter) |bloom| {
        bloom.num_elements = entry_count;
    }
    
    // Mark as initialized for proper state tracking
    sstable.is_compacted = false; // Reset compaction state
}

/// Build bloom filter from index with optimization
fn buildBloomFilterFromIndex(sstable: *SSTable) !void {
    if (sstable.bloom_filter == null) return;
    
    const bloom = sstable.bloom_filter.?;
    var added_count: usize = 0;
    
    // Add all keys to bloom filter for fast negative lookups
    for (sstable.index.items) |index_entry| {
        bloom.add(index_entry.key);
        added_count += 1;
        
        // Safety check to prevent excessive memory usage
        if (added_count > 1000000) {
            std.debug.print("⚠️ Bloom filter building stopped at 1M entries for memory safety\\n", .{});
            break;
        }
    }
    
    std.debug.print("🌸 Bloom filter built with {} keys (FPR: {:.3}%)\\n", .{
        added_count, bloom.getFalsePositiveRate() * 100.0
    });
}

// ==================== ERROR TYPES ====================

pub const LSMTreeError = error{
    NotInitialized,
    InvalidLevel,
    TooManyCompactions,
    FileOpenFailed,
    FileTruncateFailed,
    MemoryMapFailed,
    ReadBeyondFile,
    WriteBeyondFile,
    WriteToReadOnlyFile,
    SyncFailed,
    OutOfMemory,
    CorruptedFile,
    InvalidFilename,
    InvalidTableType,
    InvalidTimestamp,
    InvalidEntryCount,
    EntryTooLarge,
    IncompleteEntry,
    ReadFailed,
};