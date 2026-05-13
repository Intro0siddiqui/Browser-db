use crate::BrowserDB;
use crate::HistoryEntry;
use std::ffi::{CStr, CString};
use std::os::raw::{c_char, c_int};
use std::ptr;
use std::time::{SystemTime, UNIX_EPOCH};

#[no_mangle]
pub extern "C" fn browserdb_open(path: *const c_char) -> *mut BrowserDB {
    if path.is_null() { return ptr::null_mut(); }
    let c_str = unsafe { CStr::from_ptr(path) };
    let path_str = match c_str.to_str() {
        Ok(s) => s,
        Err(_) => return ptr::null_mut(),
    };

    match BrowserDB::open(path_str) {
        Ok(db) => Box::into_raw(Box::new(db)),
        Err(_) => ptr::null_mut(),
    }
}

#[no_mangle]
pub extern "C" fn browserdb_close(db: *mut BrowserDB) {
    if !db.is_null() {
        unsafe { drop(Box::from_raw(db)) };
    }
}

#[no_mangle]
pub extern "C" fn browserdb_history_insert(
    db: *mut BrowserDB,
    url: *const c_char,
    title: *const c_char,
    visit_count: u32
) -> c_int {
    if db.is_null() || url.is_null() || title.is_null() { return -1; }
    let db = unsafe { &*db };

    let url_str = unsafe { CStr::from_ptr(url) }.to_string_lossy().into_owned();
    let title_str = unsafe { CStr::from_ptr(title) }.to_string_lossy().into_owned();

    let url_hash = calculate_hash(&url_str);
    let entry = HistoryEntry {
        timestamp: SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_millis(),
        url: url_str,
        url_hash,
        title: title_str,
        visit_count,
    };

    match db.history().insert(&entry) {
        Ok(_) => 0,
        Err(_) => -1,
    }
}

#[no_mangle]
pub extern "C" fn browserdb_history_get_title(
    db: *mut BrowserDB,
    url_hash_low: u64,
    url_hash_high: u64
) -> *mut c_char {
    if db.is_null() { return ptr::null_mut(); }
    let db = unsafe { &*db };

    let url_hash = ((url_hash_high as u128) << 64) | (url_hash_low as u128);

    match db.history().get(url_hash) {
        Ok(Some(entry)) => {
            CString::new(entry.title).unwrap().into_raw()
        },
        _ => ptr::null_mut(),
    }
}

#[no_mangle]
pub extern "C" fn browserdb_free_string(s: *mut c_char) {
    if !s.is_null() {
        unsafe { let _ = CString::from_raw(s); };
    }
}

fn calculate_hash(s: &str) -> u128 {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    let mut s1 = DefaultHasher::new();
    s.hash(&mut s1);
    let mut s2 = DefaultHasher::new();
    "salt".hash(&mut s2);
    s.hash(&mut s2);
    ((s1.finish() as u128) << 64) | (s2.finish() as u128)
}
