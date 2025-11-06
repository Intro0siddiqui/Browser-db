const std = @import("std");
const BDBFormat = @import("../src/core/bdb_format.zig");
const testing = std.testing;

test "Complete File Validation System" {
    const allocator = testing.allocator;
    const temp_dir = "test_validation";
    
    // Setup test environment
    try std.fs.cwd().makeDir(temp_dir);
    defer std.fs.cwd().deleteDir(temp_dir) catch {};
    
    var file_manager = BDBFormat.BDBFileManager.init(allocator, temp_dir);
    defer file_manager.deinit();
    
    // Create a test file
    const test_file = try file_manager.createFile(.History);
    defer test_file.deinit();
    
    // Add some test entries
    const entry1 = BDBFormat.BDBLogEntry.createInsert("key1", "value1", std.time.milliTimestamp());
    const entry2 = BDBFormat.BDBLogEntry.createUpdate("key2", "value2", std.time.milliTimestamp());
    const entry3 = BDBFormat.BDBLogEntry.createDelete("key3", std.time.milliTimestamp());
    
    try test_file.appendEntry(entry1);
    try test_file.appendEntry(entry2);
    try test_file.appendEntry(entry3);
    
    // Test comprehensive validation
    const validation_report = try test_file.validate();
    defer validation_report.deinit();
    
    // Verify validation results
    try testing.expect(validation_report.is_valid);
    try testing.expectEqual(@as(usize, 3), validation_report.entries_validated);
    try testing.expectEqual(@as(usize, 0), validation_report.entries_corrupted);
    try testing.expect(validation_report.calculated_crc32 != 0);
    try testing.expect(validation_report.expected_crc32 != 0);
    
    std.debug.print("✅ Validation test passed: {s}\n", .{validation_report.getSummary()});
}

test "File Corruption Detection" {
    const allocator = testing.allocator;
    const temp_dir = "test_corruption";
    
    // Setup test environment
    try std.fs.cwd().makeDir(temp_dir);
    defer std.fs.cwd().deleteDir(temp_dir) catch {};
    
    var file_manager = BDBFormat.BDBFileManager.init(allocator, temp_dir);
    defer file_manager.deinit();
    
    // Create and corrupt a file
    const test_file = try file_manager.createFile(.Cookies);
    defer test_file.deinit();
    
    // Add entries
    const entry = BDBFormat.BDBLogEntry.createInsert("test", "data", std.time.milliTimestamp());
    try test_file.appendEntry(entry);
    
    // Simulate corruption by truncating file
    try test_file.file.seekFromEnd(-10);
    try test_file.file.truncate();
    
    // Test validation detects corruption
    const validation_report = try test_file.validate();
    defer validation_report.deinit();
    
    // Should detect corruption
    try testing.expect(!validation_report.is_valid);
    try testing.expect(validation_report.entries_corrupted > 0);
    
    std.debug.print("✅ Corruption detection test passed: {s}\n", .{validation_report.getSummary()});
}

test "File Repair Capability" {
    const allocator = testing.allocator;
    const temp_dir = "test_repair";
    
    // Setup test environment
    try std.fs.cwd().makeDir(temp_dir);
    defer std.fs.cwd().deleteDir(temp_dir) catch {};
    
    var file_manager = BDBFormat.BDBFileManager.init(allocator, temp_dir);
    defer file_manager.deinit();
    
    // Create a file that needs repair
    const test_file = try file_manager.createFile(.Cache);
    defer test_file.deinit();
    
    // Add entries
    const entry = BDBFormat.BDBLogEntry.createInsert("repair_test", "repair_data", std.time.milliTimestamp());
    try test_file.appendEntry(entry);
    
    // Validate before repair
    const report_before = try test_file.validate();
    defer report_before.deinit();
    
    // Attempt repair
    if (report_before.can_repair) {
        const repaired = try test_file.repair(&report_before);
        try testing.expect(repaired);
        
        // Validate after repair
        const report_after = try test_file.validate();
        defer report_after.deinit();
        
        try testing.expect(report_after.repair_successful);
        std.debug.print("✅ Repair test passed: {s}\n", .{report_after.getSummary()});
    }
}

test "BDBFileManager Validation Methods" {
    const allocator = testing.allocator;
    const temp_dir = "test_manager";
    
    // Setup test environment
    try std.fs.cwd().makeDir(temp_dir);
    defer std.fs.cwd().deleteDir(temp_dir) catch {};
    
    var file_manager = BDBFormat.BDBFileManager.init(allocator, temp_dir);
    defer file_manager.deinit();
    
    // Create multiple test files
    const file1 = try file_manager.createFile(.History);
    const file2 = try file_manager.createFile(.Cookies);
    defer file1.deinit();
    defer file2.deinit();
    
    // Add entries to files
    const entry1 = BDBFormat.BDBLogEntry.createInsert("test1", "data1", std.time.milliTimestamp());
    const entry2 = BDBFormat.BDBLogEntry.createInsert("test2", "data2", std.time.milliTimestamp());
    try file1.appendEntry(entry1);
    try file2.appendEntry(entry2);
    
    // Test validation of all files
    const reports = try file_manager.validateAllFiles(.History);
    defer {
        for (reports.items) |*report| {
            report.deinit();
        }
        reports.deinit();
    }
    
    try testing.expectEqual(@as(usize, 1), reports.items.len);
    try testing.expect(reports.items[0].is_valid);
    
    std.debug.print("✅ Manager validation test passed\n", .{});
}
