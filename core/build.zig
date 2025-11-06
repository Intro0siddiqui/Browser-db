const std = @import("std");

// Compiler目标配置
pub fn build(b: *std.Build) void {
    const target = b.standardTargetOptions(.{});
    
    // 基准版本 - 发布构建
    const exe = b.addExecutable(.{
        .name = "browserdb",
        .root_source_file = .{ .path = "src/main.zig" },
        .target = target,
        .optimize = .ReleaseSafe,
    });

    // 添加依赖
    exe.addModule("std", std.dependency("std", .{}));
    
    // 链接加密库
    exe.linkSystemLibrary("c");
    
    // 添加测试构建
    const test_step = b.step("test", "Run tests");
    
    // 单元测试
    const test_exe = b.addExecutable(.{
        .name = "browserdb-test",
        .root_source_file = .{ .path = "tests/main.zig" },
        .target = target,
        .optimize = .Debug,
    });
    
    test_exe.addModule("std", std.dependency("std", .{}));
    test_exe.linkSystemLibrary("c");
    
    // .bdb格式测试
    const bdb_format_test_exe = b.addExecutable(.{
        .name = "browserdb-bdb-format-test",
        .root_source_file = .{ .path = "tests/bdb_format_tests.zig" },
        .target = target,
        .optimize = .Debug,
    });
    
    bdb_format_test_exe.addModule("std", std.dependency("std", .{}));
    bdb_format_test_exe.linkSystemLibrary("c");
    
    // HeatMap索引系统测试
    const heatmap_test_exe = b.addExecutable(.{
        .name = "browserdb-heatmap-test",
        .root_source_file = .{ .path = "tests/heatmap_indexing_tests.zig" },
        .target = target,
        .optimize = .Debug,
    });
    
    heatmap_test_exe.addModule("std", std.dependency("std", .{}));
    heatmap_test_exe.linkSystemLibrary("c");
    
    // Modes & Operations测试
    const modes_test_exe = b.addExecutable(.{
        .name = "browserdb-modes-test",
        .root_source_file = .{ .path = "tests/modes_operations_tests.zig" },
        .target = target,
        .optimize = .Debug,
    });
    
    modes_test_exe.addModule("std", std.dependency("std", .{}));
    modes_test_exe.linkSystemLibrary("c");
    
    // LSM-Tree核心引擎测试
    const lsm_tree_test_exe = b.addExecutable(.{
        .name = "browserdb-lsm-tree-test",
        .root_source_file = .{ .path = "tests/lsm_tree_tests.zig" },
        .target = target,
        .optimize = .Debug,
    });
    
    lsm_tree_test_exe.addModule("std", std.dependency("std", .{}));
    lsm_tree_test_exe.linkSystemLibrary("c");
    
    test_step.dependOn(&test_exe.step);
    test_step.dependOn(&bdb_format_test_exe.step);
    test_step.dependOn(&heatmap_test_exe.step);
    test_step.dependOn(&modes_test_exe.step);
    test_step.dependOn(&lsm_tree_test_exe.step);

    // 性能测试
    const bench_exe = b.addExecutable(.{
        .name = "browserdb-bench",
        .root_source_file = .{ .path = "tests/bench.zig" },
        .target = target,
        .optimize = .ReleaseFast,
    });
    
    bench_exe.addModule("std", std.dependency("std", .{}));
    bench_exe.linkSystemLibrary("c");
    
    // 默认构建
    b.default_step.dependOn(&exe.step);
    
    // 安装目标
    b.installArtifact(exe);
    b.installArtifact(test_exe);
    b.installArtifact(bdb_format_test_exe);
    b.installArtifact(modes_test_exe);
    b.installArtifact(lsm_tree_test_exe);
    b.installArtifact(bench_exe);
}