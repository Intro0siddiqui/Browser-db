//! HeatMap Indexing System for BrowserDB
//! 
//! This module implements intelligent heat-based indexing that tracks query frequency,
//! access patterns, and data hotness to optimize read performance. It includes:
//! - Heat tracking algorithm for measuring access frequency
//! - Skip-list hybrid structure for heat-prioritized ordered access
//! - Dynamic heat management with decay and adaptation
//! - Bloom filters for efficient negative lookups
//! - Hot query optimization for frequently accessed data

const std = @import("std");
const mem = std.mem;
const time = std.time;
const BDB = @import("bdb_format.zig");

/// Heat tracking algorithm for measuring data access frequency and patterns
pub const HeatTracker = struct {
    const Self = @This();
    
    /// Maximum number of entries to track
    max_entries: usize,
    /// Current number of tracked entries
    current_entries: usize,
    /// Heat decay factor (0.0-1.0, lower = faster decay)
    decay_factor: f64,
    /// Heat threshold for considering data "hot"
    hot_threshold: u32,
    /// Last update timestamp for decay calculations
    last_decay_time: i64,
    
    /// Heat entry storing access frequency and timestamps
    heat_entries: std.AutoHashMap(BDB.BDBKey, HeatEntry),
    
    pub const HeatEntry = struct {
        /// Current heat level (higher = more frequently accessed)
        heat: u32,
        /// Total access count
        access_count: u32,
        /// Last access timestamp
        last_access: i64,
        /// Creation timestamp
        created_at: i64,
        /// Query pattern hash for pattern analysis
        pattern_hash: u64,
    };
    
    /// Initialize heat tracker
    pub fn init(allocator: mem.Allocator, max_entries: usize) Self {
        return Self{
            .max_entries = max_entries,
            .current_entries = 0,
            .decay_factor = 0.95, // 5% decay per decay cycle
            .hot_threshold = 10,   // Heat level to consider data hot
            .last_decay_time = time.timestamp(),
            .heat_entries = std.AutoHashMap(BDB.BDBKey, HeatEntry).init(allocator),
        };
    }
    
    /// Record data access and update heat
    pub fn recordAccess(self: *Self, key: BDB.BDBKey, query_type: QueryType) !void {
        const now = time.timestamp();
        
        // Apply decay to all entries
        self.applyDecay();
        
        // Get or create heat entry
        const entry = self.heat_entries.get(key) orelse HeatEntry{
            .heat = 0,
            .access_count = 0,
            .last_access = now,
            .created_at = now,
            .pattern_hash = 0,
        };
        
        // Calculate heat increment based on query type
        const heat_increment = switch (query_type) {
            .Read => 1,
            .Write => 2,
            .Delete => 3,
            .Compact => 4,
        };
        
        // Update heat entry
        var updated_entry = entry;
        updated_entry.heat +|= heat_increment;
        updated_entry.access_count += 1;
        updated_entry.last_access = now;
        
        // Update pattern hash for access pattern analysis
        updated_entry.pattern_hash = std.hash.Wyhash.hash(0, &key.data);
        
        self.heat_entries.put(key, updated_entry) catch return error.OutOfMemory;
    }
    
    /// Get current heat level for a key
    pub fn getHeat(self: *Self, key: BDB.BDBKey) u32 {
        const now = time.timestamp();
        if (self.heat_entries.get(key)) |entry| {
            // Apply decay to get current heat
            const age_seconds = now - entry.last_access;
            const decay_cycles = @divFloor(@as(u64, @intCast(age_seconds)), 60); // Decay every minute
            const decayed_heat = @as(u32, @intFromFloat(@as(f64, entry.heat) * 
                std.math.pow(f64, self.decay_factor, @as(f64, @floatFromInt(decay_cycles)))));
            return decayed_heat;
        }
        return 0;
    }
    
    /// Check if data is considered "hot" (frequently accessed)
    pub fn isHot(self: *Self, key: BDB.BDBKey) bool {
        return self.getHeat(key) >= self.hot_threshold;
    }
    
    /// Get top N hottest keys
    pub fn getHotKeys(self: *Self, n: usize) ![]BDB.BDBKey {
        var keys = std.ArrayList(struct {
            key: BDB.BDBKey,
            heat: u32,
        }).init(self.heat_entries.allocator);
        
        var iterator = self.heat_entries.iterator();
        while (iterator.next()) |entry| {
            const heat = self.getHeat(entry.key_ptr.*);
            try keys.append(.{
                .key = entry.key_ptr.*,
                .heat = heat,
            });
        }
        
        // Sort by heat (descending)
        mem.sort(struct {
            key: BDB.BDBKey,
            heat: u32,
        }, keys.items, {}, struct {
            fn less(_: void, a: struct { key: BDB.BDBKey, heat: u32 }, b: struct { key: BDB.BDBKey, heat: u32 }) bool {
                return a.heat > b.heat; // Descending order
            }
        });
        
        // Return top N keys
        const count = @min(n, keys.items.len);
        const result = try self.heat_entries.allocator.alloc(BDB.BDBKey, count);
        for (keys.items[0..count], 0..) |item, i| {
            result[i] = item.key;
        }
        
        return result;
    }
    
    /// Apply heat decay to all entries
    fn applyDecay(self: *Self) void {
        const now = time.timestamp();
        const time_diff = now - self.last_decay_time;
        
        // Only apply decay if enough time has passed (1 minute)
        if (time_diff < 60) return;
        
        self.last_decay_time = now;
        
        // Remove entries with very low heat to manage memory
        var keys_to_remove = std.ArrayList(BDB.BDBKey).init(self.heat_entries.allocator);
        var iterator = self.heat_entries.iterator();
        while (iterator.next()) |entry| {
            const heat = self.getHeat(entry.key_ptr.*);
            if (heat < 1) {
                keys_to_remove.append(entry.key_ptr.*) catch break;
            }
        }
        
        // Remove stale entries
        for (keys_to_remove.items) |key| {
            _ = self.heat_entries.remove(key);
            self.current_entries -= 1;
        }
    }
    
    /// Clean up and deallocate
    pub fn deinit(self: *Self) void {
        self.heat_entries.deinit();
    }
};

/// Query type for heat tracking
pub const QueryType = enum {
    Read,
    Write,
    Delete,
    Compact,
};

/// Skip-list hybrid structure for heat-prioritized ordered access
pub const HeatSkipList = struct {
    const Self = @This();
    
    /// Maximum skip-list level
    max_level: u32,
    /// Current highest level with nodes
    current_level: u32,
    /// Probability for level generation (typically 0.5)
    p: f64,
    /// Head node (sentinel)
    head: *SkipNode,
    /// Random number generator for level generation
    rng: std.Random,
    /// Heat threshold for priority queuing
    hot_threshold: u32,
    
    pub const SkipNode = struct {
        /// Key stored in this node
        key: BDB.BDBKey,
        /// Value stored with the key
        value: BDB.BDBValue,
        /// Heat level for priority scheduling
        heat: u32,
        /// Forward pointers for each level
        forward: []*SkipNode,
        /// Backward pointer for level 0
        backward: ?*SkipNode,
        
        pub fn init(allocator: mem.Allocator, key: BDB.BDBKey, value: BDB.BDBValue, max_level: u32) !*SkipNode {
            const node = try allocator.create(SkipNode);
            node.* = .{
                .key = key,
                .value = value,
                .heat = 0,
                .forward = try allocator.alloc(*SkipNode, max_level),
                .backward = null,
            };
            
            // Initialize forward pointers to null
            for (0..max_level) |i| {
                node.forward[i] = null;
            }
            
            return node;
        }
        
        pub fn deinit(self: *SkipNode, allocator: mem.Allocator) void {
            allocator.free(self.forward);
            allocator.destroy(self);
        }
    };
    
    /// Initialize heat-optimized skip-list
    pub fn init(allocator: mem.Allocator, max_level: u32) !Self {
        // Create head node
        const head = try SkipNode.init(allocator, BDB.BDBKey.empty(), BDB.BDBValue.empty(), max_level);
        
        return Self{
            .max_level = max_level,
            .current_level = 0,
            .p = 0.5, // 50% probability for next level
            .head = head,
            .rng = std.Random{ .fallback = std.random.fallback },
            .hot_threshold = 10,
        };
    }
    
    /// Insert key-value pair with heat level
    pub fn insert(self: *Self, allocator: mem.Allocator, key: BDB.BDBKey, value: BDB.BDBValue, heat: u32) !void {
        const new_node = try SkipNode.init(allocator, key, value, self.max_level);
        new_node.heat = heat;
        
        // Generate random level for new node
        const level = self.randomLevel();
        
        var update: [32]*SkipNode = undefined; // Support up to 32 levels
        var x = self.head;
        
        // Find position to insert
        for (0..self.max_level) |i| {
            if (i < self.current_level) {
                update[i] = self.head;
                x = self.head;
            } else {
                update[i] = self.head;
            }
        }
        
        for (0..level) |i| {
            const level_nodes = std.ArrayList(*SkipNode).init(allocator);
            defer level_nodes.deinit();
            
            // Collect all nodes at this level
            var node = self.head.forward[i];
            while (node) |n| {
                try level_nodes.append(n);
                node = n.forward[i];
            }
            
            // Sort by heat (hot items first)
            mem.sort(*SkipNode, level_nodes.items, {}, struct {
                fn less(_: void, a: *SkipNode, b: *SkipNode) bool {
                    return a.heat > b.heat; // Hot items first
                }
            });
            
            // Find insertion point in sorted order
            x = self.head;
            for (level_nodes.items) |node_at_level| {
                if (mem.order(u8, &node_at_level.key.data, &key.data) != .gt) {
                    break;
                }
                x = node_at_level;
            }
            
            if (x.forward[i]) |next| {
                new_node.forward[i] = next;
                next.backward = new_node;
            }
            x.forward[i] = new_node;
            new_node.backward = x;
        }
        
        // Update current level if needed
        if (level > self.current_level) {
            for (self.current_level..level) |i| {
                self.head.forward[i] = new_node;
            }
            self.current_level = level;
        }
    }
    
    /// Search for key in skip-list
    pub fn search(self: *Self, key: BDB.BDBKey) ?*SkipNode {
        var x = self.head;
        
        // Start from highest level
        for (0..self.current_level + 1) |i| {
            const level = self.current_level - i;
            while (x.forward[level]) |next| {
                if (mem.order(u8, &next.key.data, &key.data) != .lt) {
                    break;
                }
                x = next;
            }
        }
        
        x = x.forward[0] orelse return null;
        if (mem.order(u8, &x.key.data, &key.data) == .eq) {
            return x;
        }
        return null;
    }
    
    /// Get all hot keys in priority order
    pub fn getHotKeys(self: *Self, threshold: u32) ![]BDB.BDBKey {
        var hot_keys = std.ArrayList(BDB.BDBKey).init(self.rng.seed_data.allocator);
        
        var x = self.head.forward[0];
        while (x) |node| {
            if (node.heat >= threshold) {
                try hot_keys.append(node.key);
            }
            x = node.forward[0];
        }
        
        return hot_keys.toOwnedSlice();
    }
    
    /// Generate random level for new node
    fn randomLevel(self: *Self) u32 {
        var level: u32 = 0;
        while (level < self.max_level and self.rng.float(f64) < self.p) {
            level += 1;
        }
        return level;
    }
    
    /// Clean up and deallocate
    pub fn deinit(self: *Self, allocator: mem.Allocator) void {
        var current = self.head.forward[0];
        while (current) |node| {
            const next = node.forward[0];
            node.deinit(allocator);
            current = next;
        }
        self.head.deinit(allocator);
    }
};

/// Bloom filter for efficient negative lookups and cache optimization
pub const BloomFilter = struct {
    const Self = @This();
    
    /// Size of bit array in bytes
    bit_array_size: usize,
    /// Number of hash functions to use
    num_hashes: u32,
    /// Bit array for membership testing
    bit_array: []u8,
    /// Hash function type
    hash_type: HashType,
    
    pub const HashType = enum {
        Murmur3,
        FNV1a,
        DJB2,
    };
    
    /// Initialize bloom filter
    pub fn init(allocator: mem.Allocator, expected_elements: usize, false_positive_rate: f64) !Self {
        // Calculate optimal bit array size
        const optimal_bit_size = @as(usize, @intFromFloat(-(@as(f64, @floatFromInt(expected_elements)) * 
            @ln(false_positive_rate)) / std.math.ln(2.0) / std.math.ln(2.0)));
        const bit_array_size = (optimal_bit_size + 7) / 8; // Round up to bytes
        
        // Calculate optimal number of hash functions
        const k = @as(u32, @intFromFloat(@as(f64, optimal_bit_size) / 
            @as(f64, @floatFromInt(expected_elements)) * std.math.ln(2.0)));
        
        const bit_array = try allocator.alloc(u8, bit_array_size);
        // Initialize bit array to zeros
        @memset(bit_array, 0);
        
        return Self{
            .bit_array_size = bit_array_size,
            .num_hashes = @max(1, k), // At least 1 hash function
            .bit_array = bit_array,
            .hash_type = .Murmur3,
        };
    }
    
    /// Add key to bloom filter
    pub fn add(self: *Self, key: BDB.BDBKey) !void {
        const hash_value = self.hash(key);
        
        for (0..self.num_hashes) |i| {
            const bit_position = @as(usize, @intCast(hash_value % (self.bit_array_size * 8)));
            const byte_index = bit_position / 8;
            const bit_offset = bit_position % 8;
            
            self.bit_array[byte_index] |= @as(u8, 1) << @as(u3, @intCast(bit_offset));
        }
    }
    
    /// Check if key might be in set (no false negatives)
    pub fn mightContain(self: *Self, key: BDB.BDBKey) bool {
        const hash_value = self.hash(key);
        
        for (0..self.num_hashes) |i| {
            const bit_position = @as(usize, @intCast(hash_value % (self.bit_array_size * 8)));
            const byte_index = bit_position / 8;
            const bit_offset = bit_position % 8;
            
            if ((self.bit_array[byte_index] & (@as(u8, 1) << @as(u3, @intCast(bit_offset)))) == 0) {
                return false; // Definitely not in set
            }
        }
        return true; // Might be in set
    }
    
    /// Hash key using selected hash function
    fn hash(self: *Self, key: BDB.BDBKey) u64 {
        return switch (self.hash_type) {
            .Murmur3 => self.hashMurmur3(key),
            .FNV1a => self.hashFNV1a(key),
            .DJB2 => self.hashDJB2(key),
        };
    }
    
    /// MurmurHash3 implementation
    fn hashMurmur3(self: *Self, key: BDB.BDBKey) u64 {
        const seed: u32 = 0x9745b42c; // Random seed
        var hash: u64 = seed;
        var remaining = &key.data;
        
        // Process 4-byte chunks
        while (remaining.len >= 4) {
            const chunk = @as(u32, @bitCast([4]u8{
                remaining[0], remaining[1], remaining[2], remaining[3],
            }));
            
            hash = self.murmurMix(hash, chunk);
            remaining = remaining[4..];
        }
        
        // Handle remaining bytes
        var last_chunk: u32 = 0;
        for (0..remaining.len) |i| {
            last_chunk |= @as(u32, remaining[i]) << @as(u6, @intCast(i * 8));
        }
        hash = self.murmurMix(hash, last_chunk);
        
        return hash;
    }
    
    /// MurmurHash3 mix function
    fn murmurMix(self: *Self, hash: u64, chunk: u32) u64 {
        var result = hash;
        result ^= chunk;
        result = result *% 0xc6a4a7935bd1e995;
        result ^= result >> 47;
        return result;
    }
    
    /// FNV1a hash implementation
    fn hashFNV1a(self: *Self, key: BDB.BDBKey) u64 {
        var hash: u64 = 14695981039346656037; // FNV offset basis
        for (key.data) |byte| {
            hash ^= byte;
            hash = hash *% 1099511628211; // FNV prime
        }
        return hash;
    }
    
    /// DJB2 hash implementation
    fn hashDJB2(self: *Self, key: BDB.BDBKey) u64 {
        var hash: u64 = 5381;
        for (key.data) |byte| {
            hash = ((hash << 5) + hash) + byte; // hash * 33 + byte
        }
        return hash;
    }
    
    /// Get false positive rate
    pub fn getFalsePositiveRate(self: *Self) f64 {
        const m = @as(f64, @floatFromInt(self.bit_array_size * 8));
        const k = @as(f64, @floatFromInt(self.num_hashes));
        const n = @as(f64, @floatFromInt(self.getElementCount()));
        
        // Calculate theoretical false positive rate
        return std.math.pow(1.0 - std.math.exp(-k * n / m), k);
    }
    
    /// Estimate number of elements added (approximate)
    fn getElementCount(self: *Self) usize {
        var set_bits: usize = 0;
        for (self.bit_array) |byte| {
            // Count set bits
            var temp = byte;
            while (temp > 0) {
                set_bits += temp & 1;
                temp >>= 1;
            }
        }
        // Estimate based on bit density (rough approximation)
        return @divFloor(set_bits, self.num_hashes);
    }
    
    /// Clean up and deallocate
    pub fn deinit(self: *Self, allocator: mem.Allocator) void {
        allocator.free(self.bit_array);
    }
};

/// Dynamic heat manager for adaptive heat tracking
pub const DynamicHeatManager = struct {
    const Self = @This();
    
    /// Heat tracker for access frequency measurement
    heat_tracker: HeatTracker,
    /// Skip-list for heat-prioritized access
    skip_list: HeatSkipList,
    /// Bloom filter for efficient lookups
    bloom_filter: BloomFilter,
    /// Heat adaptation thresholds
    adapt_thresholds: HeatThresholds,
    
    pub const HeatThresholds = struct {
        /// Heat level for considering data "hot"
        hot_threshold: u32,
        /// Heat level for considering data "very hot"
        very_hot_threshold: u32,
        /// Minimum heat to keep in skip-list
        min_heat: u32,
        /// Decay factor for heat decay
        decay_factor: f64,
    };
    
    /// Initialize dynamic heat manager
    pub fn init(allocator: mem.Allocator, max_entries: usize, skip_levels: u32) !Self {
        const heat_tracker = HeatTracker.init(allocator, max_entries);
        const skip_list = try HeatSkipList.init(allocator, skip_levels);
        const bloom_filter = try BloomFilter.init(allocator, max_entries, 0.01); // 1% false positive rate
        
        return Self{
            .heat_tracker = heat_tracker,
            .skip_list = skip_list,
            .bloom_filter = bloom_filter,
            .adapt_thresholds = HeatThresholds{
                .hot_threshold = 10,
                .very_hot_threshold = 50,
                .min_heat = 1,
                .decay_factor = 0.95,
            },
        };
    }
    
    /// Record access and update all heat structures
    pub fn recordAccess(self: *Self, key: BDB.BDBKey, value: BDB.BDBValue, query_type: QueryType) !void {
        // Record in heat tracker
        try self.heat_tracker.recordAccess(key, query_type);
        
        // Get updated heat level
        const heat = self.heat_tracker.getHeat(key);
        
        // Add to skip-list if heat is above minimum
        if (heat >= self.adapt_thresholds.min_heat) {
            try self.skip_list.insert(self.heat_tracker.heat_entries.allocator, key, value, heat);
        }
        
        // Add to bloom filter
        try self.bloom_filter.add(key);
    }
    
    /// Check if key might be present using bloom filter
    pub fn mightContain(self: *Self, key: BDB.BDBKey) bool {
        return self.bloom_filter.mightContain(key);
    }
    
    /// Get heat level for key
    pub fn getHeat(self: *Self, key: BDB.BDBKey) u32 {
        return self.heat_tracker.getHeat(key);
    }
    
    /// Check if key is hot
    pub fn isHot(self: *Self, key: BDB.BDBKey) bool {
        return self.heat_tracker.isHot(key);
    }
    
    /// Get hot keys from skip-list
    pub fn getHotKeys(self: *Self, count: usize) ![]BDB.BDBKey {
        return self.skip_list.getHotKeys(self.adapt_thresholds.hot_threshold);
    }
    
    /// Adapt thresholds based on access patterns
    pub fn adaptThresholds(self: *Self) void {
        const hot_keys = self.heat_tracker.getHotKeys(100) catch return;
        defer self.heat_tracker.heat_entries.allocator.free(hot_keys);
        
        if (hot_keys.len == 0) return;
        
        // Analyze heat distribution
        var total_heat: u64 = 0;
        for (hot_keys) |key| {
            total_heat += self.heat_tracker.getHeat(key);
        }
        
        const avg_heat = @as(u32, @intCast(total_heat / hot_keys.len));
        
        // Adapt thresholds based on distribution
        if (avg_heat > 50) {
            // Increase thresholds if average is very high
            self.adapt_thresholds.hot_threshold = @max(20, self.adapt_thresholds.hot_threshold + 5);
        } else if (avg_heat < 5) {
            // Decrease thresholds if average is very low
            self.adapt_thresholds.hot_threshold = @max(5, self.adapt_thresholds.hot_threshold - 2);
        }
    }
    
    /// Clean up and deallocate
    pub fn deinit(self: *Self, allocator: mem.Allocator) void {
        self.heat_tracker.deinit();
        self.skip_list.deinit(allocator);
        self.bloom_filter.deinit(allocator);
    }
};

/// Integration with BrowserDB core for heat-aware operations
pub const HeatAwareBrowserDB = struct {
    const Self = @This();
    
    /// Underlying BrowserDB core (integrated from previous phases)
    core: *BDB.BrowserDB,
    /// Dynamic heat management system
    heat_manager: DynamicHeatManager,
    /// Cache for frequently accessed data
    hot_cache: HotCache,
    
    pub const HotCache = struct {
        /// Maximum cache size in entries
        max_size: usize,
        /// Current cache size
        current_size: usize,
        /// Cache entries
        entries: std.AutoHashMap(BDB.BDBKey, CacheEntry),
        
        pub const CacheEntry = struct {
            value: BDB.BDBValue,
            heat: u32,
            last_access: i64,
            access_count: u32,
        };
        
        pub fn init(allocator: mem.Allocator, max_size: usize) Self {
            return Self{
                .max_size = max_size,
                .current_size = 0,
                .entries = std.AutoHashMap(BDB.BDBKey, CacheEntry).init(allocator),
            };
        }
        
        pub fn get(self: *Self, key: BDB.BDBKey) ?*CacheEntry {
            return self.entries.get(key);
        }
        
        pub fn put(self: *Self, key: BDB.BDBKey, value: BDB.BDBValue, heat: u32) !void {
            const now = time.timestamp();
            
            // Evict lowest heat entry if cache is full
            if (self.current_size >= self.max_size) {
                self.evictLowestHeat();
            }
            
            const entry = CacheEntry{
                .value = value,
                .heat = heat,
                .last_access = now,
                .access_count = 1,
            };
            
            try self.entries.put(key, entry);
            self.current_size += 1;
        }
        
        fn evictLowestHeat(self: *Self) void {
            var min_key: ?BDB.BDBKey = null;
            var min_heat: u32 = std.math.maxInt(u32);
            
            var iterator = self.entries.iterator();
            while (iterator.next()) |entry| {
                if (entry.value_ptr.heat < min_heat) {
                    min_heat = entry.value_ptr.heat;
                    min_key = entry.key_ptr.*;
                }
            }
            
            if (min_key) |key| {
                _ = self.entries.remove(key);
                self.current_size -= 1;
            }
        }
        
        pub fn deinit(self: *Self) void {
            self.entries.deinit();
        }
    };
    
    /// Initialize heat-aware BrowserDB
    pub fn init(allocator: mem.Allocator, core: *BDB.BrowserDB, max_heat_entries: usize, cache_size: usize) !Self {
        const heat_manager = try DynamicHeatManager.init(allocator, max_heat_entries, 16);
        const hot_cache = HotCache.init(allocator, cache_size);
        
        return Self{
            .core = core,
            .heat_manager = heat_manager,
            .hot_cache = hot_cache,
        };
    }
    
    /// Heat-aware get operation
    pub fn get(self: *Self, key: BDB.BDBKey) !BDB.BDBValue {
        // First check hot cache
        if (self.hot_cache.get(key)) |cached_entry| {
            const now = time.timestamp();
            // Update access patterns
            cached_entry.last_access = now;
            cached_entry.access_count += 1;
            
            // Record access in heat system
            try self.heat_manager.recordAccess(key, cached_entry.value, .Read);
            
            return cached_entry.value;
        }
        
        // Check if key might exist using bloom filter
        if (!self.heat_manager.mightContain(key)) {
            // Definitely not in database, return empty value
            return BDB.BDBValue.empty();
        }
        
        // Search in skip-list for hot keys
        if (self.heat_manager.skip_list.search(key)) |node| {
            const value = node.value;
            // Add to cache if heat is high enough
            const heat = self.heat_manager.getHeat(key);
            if (heat >= self.heat_manager.adapt_thresholds.hot_threshold) {
                try self.hot_cache.put(key, value, heat);
            }
            return value;
        }
        
        // Fall back to core database search
        const value = try self.core.get(key);
        
        // Record access if found
        if (value.data.len > 0) {
            try self.heat_manager.recordAccess(key, value, .Read);
        }
        
        return value;
    }
    
    /// Heat-aware put operation
    pub fn put(self: *Self, key: BDB.BDBKey, value: BDB.BDBValue) !void {
        // Record access in heat system
        try self.heat_manager.recordAccess(key, value, .Write);
        
        // Update core database
        try self.core.put(key, value);
        
        // Add to hot cache if heat is high
        const heat = self.heat_manager.getHeat(key);
        if (heat >= self.heat_manager.adapt_thresholds.hot_threshold) {
            try self.hot_cache.put(key, value, heat);
        }
    }
    
    /// Get performance statistics
    pub fn getStats(self: *Self) HeatStats {
        return HeatStats{
            .heat_entries = self.heat_manager.heat_tracker.current_entries,
            .hot_keys = self.hot_cache.current_size,
            .bloom_filter_elements = self.heat_manager.bloom_filter.getElementCount(),
            .false_positive_rate = self.heat_manager.bloom_filter.getFalsePositiveRate(),
            .adapted_thresholds = self.heat_manager.adapt_thresholds,
        };
    }
    
    /// Clean up and deallocate
    pub fn deinit(self: *Self, allocator: mem.Allocator) void {
        self.heat_manager.deinit(allocator);
        self.hot_cache.deinit();
    }
};

/// Statistics for heat indexing system
pub const HeatStats = struct {
    /// Number of heat entries tracked
    heat_entries: usize,
    /// Number of hot keys cached
    hot_keys: usize,
    /// Estimated number of elements in bloom filter
    bloom_filter_elements: usize,
    /// Current false positive rate of bloom filter
    false_positive_rate: f64,
    /// Current adaptive thresholds
    adapted_thresholds: DynamicHeatManager.HeatThresholds,
};

test "HeatTracker basic functionality" {
    const allocator = std.testing.allocator;
    
    var tracker = HeatTracker.init(allocator, 1000);
    defer tracker.deinit();
    
    const test_key = BDB.BDBKey.fromString("test_key") catch return;
    
    // Initially no heat
    try std.testing.expectEqual(@as(u32, 0), tracker.getHeat(test_key));
    try std.testing.expectEqual(false, tracker.isHot(test_key));
    
    // Record access
    try tracker.recordAccess(test_key, .Read);
    try std.testing.expect(tracker.getHeat(test_key) > 0);
}

test "BloomFilter basic functionality" {
    const allocator = std.testing.allocator;
    
    var filter = try BloomFilter.init(allocator, 1000, 0.01);
    defer filter.deinit();
    
    const test_key = BDB.BDBKey.fromString("test_key") catch return;
    
    // Initially should not contain key
    try std.testing.expectEqual(false, filter.mightContain(test_key));
    
    // Add key
    try filter.add(test_key);
    
    // Should now contain key
    try std.testing.expectEqual(true, filter.mightContain(test_key));
}

test "HeatAwareBrowserDB integration" {
    const allocator = std.testing.allocator;
    
    // This test would integrate with the core BrowserDB
    // For now, we'll test the heat manager independently
    var heat_manager = try DynamicHeatManager.init(allocator, 1000, 16);
    defer heat_manager.deinit(allocator);
    
    const test_key = BDB.BDBKey.fromString("test_key") catch return;
    const test_value = BDB.BDBValue.fromString("test_value") catch return;
    
    // Record access
    try heat_manager.recordAccess(test_key, test_value, .Read);
    
    // Should have heat now
    const heat = heat_manager.getHeat(test_key);
    try std.testing.expect(heat > 0);
}