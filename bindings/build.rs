//! BrowserDB构建脚本
//! 
//! 这个脚本负责编译Zig核心库并链接到Rust包中。

use std::env;
use std::fs;
use std::path::PathBuf;

fn main() {
    println!("cargo:rerun-if-env-changed=BROWSERDB_ZIG_PATH");
    
    // 获取Zig编译器路径
    let zig_path = env::var("BROWSERDB_ZIG_PATH").unwrap_or_else(|_| "zig".to_string());
    
    // 获取当前目录
    let project_root = env::var("CARGO_MANIFEST_DIR").unwrap();
    let project_root_path = PathBuf::from(&project_root);
    
    // Zig源码路径
    let zig_src_path = project_root_path.parent().unwrap().join("core");
    
    // 确保Zig源码存在
    if !zig_src_path.exists() {
        panic!("Zig core source not found at: {}", zig_src_path.display());
    }
    
    // 构建Zig库
    println!("🔧 Building Zig core engine...");
    
    let build_result = std::process::Command::new(&zig_path)
        .args(&["build-lib", "-Drelease-safe", "-femit-bin=browserdb.o"])
        .current_dir(&zig_src_path)
        .output()
        .expect("Failed to execute Zig build command");
    
    if !build_result.status.success() {
        panic!("Zig build failed: {}", String::from_utf8_lossy(&build_result.stderr));
    }
    
    // 查找生成的库文件
    let lib_path = zig_src_path.join("zig-out").join("lib").join("libbrowserdb.a");
    
    if !lib_path.exists() {
        panic!("BrowserDB library not found at: {}", lib_path.display());
    }
    
    println!("✅ Zig library built: {}", lib_path.display());
    
    // 告诉链接器链接到生成的库
    println!("cargo:rustc-link-search=native={}", lib_path.parent().unwrap().display());
    println!("cargo:rustc-link-lib=static=browserdb");
    
    // 如果可用，添加优化标志
    if env::var("OPT_LEVEL").unwrap_or_else(|_| "0".to_string()) != "0" {
        println!("cargo:rustc-link-arg=-s"); // 剥离调试信息
    }
    
    // 设置特性标志
    if env::var("CARGO_FEATURE_PERFORMANCE").is_ok() {
        println!("cargo:features=performance");
    }
    
    if env::var("CARGO_FEATURE_VERBOSE_LOGGING").is_ok() {
        println!("cargo:features=verbose-logging");
    }
}