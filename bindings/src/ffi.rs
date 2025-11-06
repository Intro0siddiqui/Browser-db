//! FFI (Foreign Function Interface) bindings to Zig core
//! 
//! This module handles the low-level interface between Rust and the 
//! high-performance Zig core engine.

use std::ffi::{CStr, CString};

#[link(name = "browserdb", kind = "static")]
extern "C" {
    /// 初始化BrowserDB核心引擎
    pub fn browserdb_init(path: *const u8, path_len: usize) -> ::std::os::raw::c_int;
    
    /// 清理BrowserDB资源
    pub fn browserdb_cleanup();
    
    /// 清理整个数据库
    pub fn browserdb_wipe(path: *const u8, path_len: usize) -> ::std::os::raw::c_int;
    
    /// 插入历史记录
    pub fn browserdb_history_insert(
        timestamp: u64,
        url_hash_lo: u64,
        url_hash_hi: u64,
        title: *const u8,
        title_len: usize,
    ) -> ::std::os::raw::c_int;
    
    /// 获取历史记录
    pub fn browserdb_history_get(
        url_hash_lo: u64,
        url_hash_hi: u64,
        result: *mut u8,
        result_len: *mut usize,
    ) -> ::std::os::raw::c_int;
    
    /// 插入Cookie
    pub fn browserdb_cookies_insert(
        domain_hash_lo: u64,
        domain_hash_hi: u64,
        name: *const u8,
        name_len: usize,
        value: *const u8,
        value_len: usize,
        expiry: u64,
        flags: u8,
    ) -> ::std::os::raw::c_int;
    
    /// 插入缓存条目
    pub fn browserdb_cache_insert(
        url_hash_lo: u64,
        url_hash_hi: u64,
        headers: *const u8,
        headers_len: usize,
        body: *const u8,
        body_len: usize,
        etag: *const u8,
        etag_len: usize,
        last_modified: u64,
    ) -> ::std::os::raw::c_int;
    
    /// 获取数据库统计信息
    pub fn browserdb_stats(
        total_entries: *mut u64,
        memory_usage: *mut u64,
        disk_usage: *mut u64,
    ) -> ::std::os::raw::c_int;
}

// 错误处理
pub type BrowserDBResult = ::std::os::raw::c_int;

// 错误码
pub const BROWSERDB_SUCCESS: BrowserDBResult = 0;
pub const BROWSERDB_ERROR_PATH: BrowserDBResult = -1;
pub const BROWSERDB_ERROR_IO: BrowserDBResult = -2;
pub const BROWSERDB_ERROR_MEMORY: BrowserDBResult = -3;
pub const BROWSERDB_ERROR_CORRUPTION: BrowserDBResult = -4;

// 安全包装函数
pub fn init_database(path: &str) -> Result<(), String> {
    let c_path = match CString::new(path) {
        Ok(c) => c,
        Err(_) => return Err("Invalid path".to_string()),
    };
    
    let result = unsafe { browserdb_init(c_path.as_ptr(), c_path.as_bytes().len()) };
    
    match result {
        BROWSERDB_SUCCESS => Ok(()),
        BROWSERDB_ERROR_PATH => Err("Invalid path".to_string()),
        BROWSERDB_ERROR_IO => Err("I/O error".to_string()),
        BROWSERDB_ERROR_MEMORY => Err("Memory error".to_string()),
        _ => Err("Unknown error".to_string()),
    }
}

pub fn wipe_database(path: &str) -> Result<(), String> {
    let c_path = match CString::new(path) {
        Ok(c) => c,
        Err(_) => return Err("Invalid path".to_string()),
    };
    
    let result = unsafe { browserdb_wipe(c_path.as_ptr(), c_path.as_bytes().len()) };
    
    match result {
        BROWSERDB_SUCCESS => Ok(()),
        BROWSERDB_ERROR_PATH => Err("Invalid path".to_string()),
        BROWSERDB_ERROR_IO => Err("I/O error".to_string()),
        _ => Err("Unknown error".to_string()),
    }
}

pub fn insert_history_entry(timestamp: u64, url_hash: u128, title: &str) -> Result<(), String> {
    let c_title = match CString::new(title) {
        Ok(c) => c,
        Err(_) => return Err("Invalid title".to_string()),
    };
    
    let (lo, hi) = split_u128(url_hash);
    
    let result = unsafe {
        browserdb_history_insert(
            timestamp,
            lo,
            hi,
            c_title.as_ptr(),
            c_title.as_bytes().len(),
        )
    };
    
    match result {
        BROWSERDB_SUCCESS => Ok(()),
        BROWSERDB_ERROR_IO => Err("I/O error".to_string()),
        BROWSERDB_ERROR_MEMORY => Err("Memory error".to_string()),
        _ => Err("Unknown error".to_string()),
    }
}

pub fn insert_cookie(
    domain_hash: u128,
    name: &str,
    value: &str,
    expiry: u64,
    flags: u8,
) -> Result<(), String> {
    let c_name = match CString::new(name) {
        Ok(c) => c,
        Err(_) => return Err("Invalid cookie name".to_string()),
    };
    
    let c_value = match CString::new(value) {
        Ok(c) => c,
        Err(_) => return Err("Invalid cookie value".to_string()),
    };
    
    let (domain_lo, domain_hi) = split_u128(domain_hash);
    
    let result = unsafe {
        browserdb_cookies_insert(
            domain_lo,
            domain_hi,
            c_name.as_ptr(),
            c_name.as_bytes().len(),
            c_value.as_ptr(),
            c_value.as_bytes().len(),
            expiry,
            flags,
        )
    };
    
    match result {
        BROWSERDB_SUCCESS => Ok(()),
        BROWSERDB_ERROR_IO => Err("I/O error".to_string()),
        BROWSERDB_ERROR_MEMORY => Err("Memory error".to_string()),
        _ => Err("Unknown error".to_string()),
    }
}

pub fn get_stats() -> Result<(u64, u64, u64), String> {
    let mut total_entries: u64 = 0;
    let mut memory_usage: u64 = 0;
    let mut disk_usage: u64 = 0;
    
    let result = unsafe {
        browserdb_stats(
            &mut total_entries,
            &mut memory_usage,
            &mut disk_usage,
        )
    };
    
    match result {
        BROWSERDB_SUCCESS => Ok((total_entries, memory_usage, disk_usage)),
        BROWSERDB_ERROR_IO => Err("I/O error".to_string()),
        _ => Err("Unknown error".to_string()),
    }
}

// 工具函数：将u128分割为两个u64
fn split_u128(value: u128) -> (u64, u64) {
    let lo = (value & 0xFFFFFFFFFFFFFFFF) as u64;
    let hi = (value >> 64) as u64;
    (lo, hi)
}