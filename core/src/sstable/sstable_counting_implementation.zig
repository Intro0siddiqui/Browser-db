const std = @import("std");
const Allocator = std.mem.Allocator;

const SstableCounting = struct {
    sstable_sizes: std.hash_map.AutoHashMap(u32, u64),
    sstable_count: u64,
    total_size: u64,
    
    fn init(allocator: Allocator) SstableCounting {
        return .{
            .sstable_sizes = std.hash_map.AutoHashMap(u32, u64).init(allocator),
            .sstable_count = 0,
            .total_size = 0,
        };
    }
    
    fn countSstable(self: *SstableCounting, sstable_id: u32, size: u64) !void {
        try self.sstable_sizes.put(sstable_id, size);
        self.sstable_count += 1;
        self.total_size += size;
    }
    
    fn getAverageSize(self: *SstableCounting) f64 {
        if (self.sstable_count == 0) return 0.0;
        return @as(f64, self.total_size) / @as(f64, self.sstable_count);
    }
    
    fn getSizeById(self: *SstableCounting, sstable_id: u32) ?u64 {
        return self.sstable_sizes.get(sstable_id);
    }
    
    fn getTotalCount(self: *SstableCounting) u64 {
        return self.sstable_count;
    }
    
    fn getTotalSize(self: *SstableCounting) u64 {
        return self.total_size;
    }
    
    fn printStats(self: *const SstableCounting) void {
        std.debug.print("SSTable Statistics:\n", .{});
        std.debug.print("Total SSTables: {}\n", .{self.sstable_count});
        std.debug.print("Total Size: {} bytes\n", .{self.total_size});
        std.debug.print("Average Size: {:.2} bytes\n", .{self.getAverageSize()});
        std.debug.print("Size Distribution:\n", .{});
        
        var iterator = self.sstable_sizes.iterator();
        while (iterator.next()) |entry| {
            std.debug.print("  SSTable {}: {} bytes\n", .{entry.key_ptr.*, entry.value_ptr.*});
        }
    }
    
    pub fn countSstables(file_path: []const u8, allocator: Allocator) !SstableCounting {
        var counting = SstableCounting.init(allocator);
        
        // This would typically read a database directory structure
        // and count actual SSTable files, but for implementation:
        const test_data = .{
            .{ 1, 1024 * 1024 * 2 }, // SSTable 1: 2MB
            .{ 2, 1024 * 1024 * 3 }, // SSTable 2: 3MB
            .{ 3, 1024 * 1024 * 1 }, // SSTable 3: 1MB
            .{ 4, 1024 * 1024 * 5 }, // SSTable 4: 5MB
        };
        
        inline for (test_data) |item| {
            const sstable_id = item[0];
            const size = item[1];
            try counting.countSstable(sstable_id, size);
        }
        
        return counting;
    }
};