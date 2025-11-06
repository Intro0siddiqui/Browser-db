const std = @import("std");
const BrowserDB = @import("core/browserdb.zig");

pub fn main() !void {
    var gpa = std.heap.GeneralPurposeAllocator(.{}){};
    defer _ = gpa.deinit();
    
    const allocator = gpa.allocator();
    
    std.debug.print("🚀 BrowserDB Core Engine v0.1.0\n", .{});
    std.debug.print("High-performance LSM-Tree database for browsers\n\n", .{});
    
    // 初始化数据库
    const db = try BrowserDB.init(allocator, "/tmp/.browserdb");
    defer db.deinit();
    
    std.debug.print("✅ Database initialized at /tmp/.browserdb\n", .{});
    
    // 基本性能测试
    try runBasicTest(&db);
    
    std.debug.print("\n🎉 All tests passed! BrowserDB is ready.\n", .{});
}

fn runBasicTest(db: *const BrowserDB) !void {
    std.debug.print("Running basic functionality test...\n", .{});
    
    // 测试写入性能
    const start_time = std.time.nanoTimestamp();
    for (0..1000) |i| {
        const entry = BrowserDB.HistoryEntry{
            .timestamp = std.time.milliTimestamp(),
            .url_hash = @as(u128, @intCast(i)),
            .title = "Test Entry",
            .visit_count = 1,
        };
        try db.history.insert(entry);
    }
    const end_time = std.time.nanoTimestamp();
    const elapsed = @as(f64, @floatFromInt(end_time - start_time)) / 1_000_000.0;
    
    std.debug.print("📊 Wrote 1000 entries in {:.2}ms\n", .{elapsed});
    std.debug.print("   Performance: {:.0} writes/sec\n", .{1000.0 / (elapsed / 1000.0)});
    
    // 测试读取性能
    const read_start = std.time.nanoTimestamp();
    for (0..100) |i| {
        _ = try db.history.get(@as(u128, @intCast(i)));
    }
    const read_end = std.time.nanoTimestamp();
    const read_elapsed = @as(f64, @floatFromInt(read_end - read_start)) / 1_000_000.0;
    
    std.debug.print("📊 Read 100 entries in {:.2}ms\n", .{read_elapsed});
    std.debug.print("   Performance: {:.0} reads/sec\n", .{100.0 / (read_elapsed / 1000.0)});
}