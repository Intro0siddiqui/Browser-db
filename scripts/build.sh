#!/bin/bash

# ZawraDB 构建脚本
# 自动化构建整个项目

set -e  # 遇到错误立即退出

echo "🚀 ZawraDB 构建脚本"
echo "========================"

# 检查依赖
echo "🔍 检查依赖..."

if ! command -v zig &> /dev/null; then
    echo "❌ Zig 未安装，请安装 Zig 0.13.0 或更高版本"
    exit 1
fi

if ! command -v cargo &> /dev/null; then
    echo "❌ Rust/Cargo 未安装，请安装 Rust 1.75 或更高版本"
    exit 1
fi

ZIG_VERSION=$(zig version)
RUST_VERSION=$(rustc --version | cut -d' ' -f2)

echo "✅ Zig 版本: $ZIG_VERSION"
echo "✅ Rust 版本: $RUST_VERSION"

# 清理之前的构建
echo "🧹 清理之前的构建..."
cd core && rm -rf zig-out build *.o *.a 2>/dev/null || true
cd ../bindings && rm -rf target 2>/dev/null || true
cd ..

# 构建 Zig 核心
echo "🔨 构建 Zig 核心引擎..."
cd core
echo "  使用模式: ReleaseSafe"
zig build -Drelease-safe
if [ $? -eq 0 ]; then
    echo "  ✅ Zig 核心构建成功"
else
    echo "  ❌ Zig 核心构建失败"
    exit 1
fi

# 运行 Zig 测试
echo "🧪 运行 Zig 核心测试..."
zig build test
if [ $? -eq 0 ]; then
    echo "  ✅ Zig 测试通过"
else
    echo "  ❌ Zig 测试失败"
    echo "  ⚠️  继续构建 Rust 绑定..."
fi

# 构建基准测试
echo "⚡ 构建性能基准测试..."
zig build -Drelease-fast -femit-bin=zawradb-bench
if [ $? -eq 0 ]; then
    echo "  ✅ 基准测试构建成功"
else
    echo "  ⚠️  基准测试构建失败"
fi

cd ..

# 构建 Rust 绑定
echo "🔨 构建 Rust 绑定..."
cd bindings

# 设置环境变量
export ZAWRADB_ZIG_PATH=$(which zig)

# 清理 Rust 构建缓存
cargo clean

# 构建发布版本
echo "  使用模式: Release"
cargo build --release
if [ $? -eq 0 ]; then
    echo "  ✅ Rust 绑定构建成功"
else
    echo "  ❌ Rust 绑定构建失败"
    exit 1
fi

# 运行 Rust 测试
echo "🧪 运行 Rust 绑定测试..."
cargo test --release
if [ $? -eq 0 ]; then
    echo "  ✅ Rust 测试通过"
else
    echo "  ❌ Rust 测试失败"
    echo "  ⚠️  一些测试可能需要实际的 Zig 库"
fi

cd ..

# 构建示例
echo "🔨 构建示例程序..."
cd examples
cargo build --release
if [ $? -eq 0 ]; then
    echo "  ✅ 示例程序构建成功"
else
    echo "  ⚠️  示例程序构建失败"
fi

cd ..

# 运行快速基准测试
echo "⚡ 运行快速基准测试..."
if [ -f "core/zig-out/bin/zawradb-bench" ]; then
    echo "  执行基准测试..."
    ./core/zig-out/bin/zawradb-bench || echo "  ⚠️  基准测试执行失败"
else
    echo "  ⚠️  基准测试可执行文件不存在"
fi

echo ""
echo "🎉 构建完成！"
echo ""
echo "📦 可执行文件:"
if [ -f "core/zig-out/bin/zawradb" ]; then
    echo "  - 核心引擎: core/zig-out/bin/zawradb"
fi
if [ -f "core/zig-out/bin/zawradb-bench" ]; then
    echo "  - 基准测试: core/zig-out/bin/zawradb-bench"
fi
if [ -f "bindings/target/release/zawradb" ]; then
    echo "  - Rust 绑定: bindings/target/release/zawradb"
fi
if [ -f "examples/target/release/basic_usage" ]; then
    echo "  - 示例程序: examples/target/release/basic_usage"
fi

echo ""
echo "🚀 快速开始:"
echo "  cd core && ./zig-out/bin/zawradb"
echo "  cd bindings && cargo run --example basic_usage"

# 生成构建报告
echo ""
echo "📊 构建报告:"