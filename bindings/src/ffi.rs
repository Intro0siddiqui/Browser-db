use crate::BrowserDB;
use crate::HistoryEntry;
use crate::LocalStoreEntry;
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

#[no_mangle]
pub extern "C" fn browserdb_localstore_insert(
    db: *mut BrowserDB,
    origin: *const c_char,
    key: *const c_char,
    value: *const c_char,
) -> c_int {
    if db.is_null() || origin.is_null() || key.is_null() || value.is_null() { return -1; }
    let db = unsafe { &*db };
    let origin_str = unsafe { CStr::from_ptr(origin) }.to_string_lossy().into_owned();
    let key_str = unsafe { CStr::from_ptr(key) }.to_string_lossy().into_owned();
    let value_str = unsafe { CStr::from_ptr(value) }.to_string_lossy().into_owned();

    let origin_hash = calculate_hash(&origin_str);
    let entry = LocalStoreEntry {
        origin_hash,
        key: key_str,
        value: value_str,
    };

    match db.localstore().insert(&entry) {
        Ok(_) => 0,
        Err(_) => -1,
    }
}

#[no_mangle]
pub extern "C" fn browserdb_localstore_get(
    db: *mut BrowserDB,
    origin: *const c_char,
    key: *const c_char,
) -> *mut c_char {
    if db.is_null() || origin.is_null() || key.is_null() { return ptr::null_mut(); }
    let db = unsafe { &*db };
    let origin_str = unsafe { CStr::from_ptr(origin) }.to_string_lossy().into_owned();
    let key_str = unsafe { CStr::from_ptr(key) }.to_string_lossy().into_owned();

    let origin_hash = calculate_hash(&origin_str);
    match db.localstore().get_by_origin(origin_hash) {
        Ok(entries) => {
            for entry in entries {
                if entry.key == key_str {
                    return CString::new(entry.value).unwrap().into_raw();
                }
            }
            ptr::null_mut()
        }
        _ => ptr::null_mut(),
    }
}

#[no_mangle]
pub extern "C" fn browserdb_localstore_remove(
    db: *mut BrowserDB,
    origin: *const c_char,
    key: *const c_char,
) -> c_int {
    if db.is_null() || origin.is_null() || key.is_null() { return -1; }
    let db = unsafe { &*db };
    let origin_str = unsafe { CStr::from_ptr(origin) }.to_string_lossy().into_owned();
    let key_str = unsafe { CStr::from_ptr(key) }.to_string_lossy().into_owned();
    let origin_hash = calculate_hash(&origin_str);

    let current_mode = db.localstore().container.switcher.current_mode.read();
    let key_bytes = match bincode::serialize(&(origin_hash, &key_str)) {
        Ok(k) => k,
        Err(_) => return -1,
    };

    match &*current_mode {
        crate::core::modes::CurrentMode::Persistent(pm) => {
            let _ = pm.localstore.delete(key_bytes);
        }
        crate::core::modes::CurrentMode::Ultra(um) => {
            um.localstore.delete(&key_bytes);
        }
    }
    0
}

#[no_mangle]
pub extern "C" fn browserdb_localstore_clear(
    db: *mut BrowserDB,
    origin: *const c_char,
) -> c_int {
    if db.is_null() || origin.is_null() { return -1; }
    let db = unsafe { &*db };
    let origin_str = unsafe { CStr::from_ptr(origin) }.to_string_lossy().into_owned();
    let origin_hash = calculate_hash(&origin_str);

    let current_mode = db.localstore().container.switcher.current_mode.read();
    match db.localstore().get_by_origin(origin_hash) {
        Ok(entries) => {
            for entry in entries {
                let key_bytes = bincode::serialize(&(origin_hash, &entry.key)).unwrap();
                match &*current_mode {
                    crate::core::modes::CurrentMode::Persistent(pm) => {
                        let _ = pm.localstore.delete(key_bytes);
                    }
                    crate::core::modes::CurrentMode::Ultra(um) => {
                        um.localstore.delete(&key_bytes);
                    }
                }
            }
            0
        }
        _ => -1,
    }
}

#[no_mangle]
pub extern "C" fn browserdb_localstore_get_all(
    db: *mut BrowserDB,
    origin: *const c_char,
    callback: extern "C" fn(*const c_char, *const c_char, *mut std::ffi::c_void),
    user_data: *mut std::ffi::c_void,
) -> c_int {
    if db.is_null() || origin.is_null() { return -1; }
    let db = unsafe { &*db };
    let origin_str = unsafe { CStr::from_ptr(origin) }.to_string_lossy().into_owned();
    let origin_hash = calculate_hash(&origin_str);

    match db.localstore().get_by_origin(origin_hash) {
        Ok(entries) => {
            for entry in entries {
                let k_c = CString::new(entry.key).unwrap();
                let v_c = CString::new(entry.value).unwrap();
                callback(k_c.as_ptr(), v_c.as_ptr(), user_data);
            }
            0
        }
        _ => -1,
    }
}

#[no_mangle]
pub extern "C" fn browserdb_settings_set(
    db: *mut BrowserDB,
    key: *const c_char,
    value: *const c_char,
) -> c_int {
    if db.is_null() || key.is_null() || value.is_null() { return -1; }
    let db = unsafe { &*db };
    let key_str = unsafe { CStr::from_ptr(key) }.to_string_lossy();
    let value_str = unsafe { CStr::from_ptr(value) }.to_string_lossy();

    match db.settings().set(&key_str, &value_str) {
        Ok(_) => 0,
        Err(_) => -1,
    }
}

#[no_mangle]
pub extern "C" fn browserdb_settings_get(
    db: *mut BrowserDB,
    key: *const c_char,
) -> *mut c_char {
    if db.is_null() || key.is_null() { return ptr::null_mut(); }
    let db = unsafe { &*db };
    let key_str = unsafe { CStr::from_ptr(key) }.to_string_lossy();

    match db.settings().get(&key_str) {
        Ok(Some(val)) => CString::new(val).unwrap().into_raw(),
        _ => ptr::null_mut(),
    }
}

#[no_mangle]
pub extern "C" fn browserdb_settings_remove(
    db: *mut BrowserDB,
    key: *const c_char,
) -> c_int {
    if db.is_null() || key.is_null() { return -1; }
    let db = unsafe { &*db };
    let key_bytes = unsafe { CStr::from_ptr(key) }.to_bytes().to_vec();

    let current_mode = db.settings().container.switcher.current_mode.read();
    match &*current_mode {
        crate::core::modes::CurrentMode::Persistent(pm) => {
            let _ = pm.settings.delete(key_bytes);
        }
        crate::core::modes::CurrentMode::Ultra(um) => {
            um.settings.delete(&key_bytes);
        }
    }
    0
}

#[no_mangle]
pub extern "C" fn browserdb_localstore_increment(
    db: *mut BrowserDB,
    origin: *const c_char,
    key: *const c_char,
    delta: i64,
) -> c_int {
    if db.is_null() || origin.is_null() || key.is_null() { return -1; }
    let db = unsafe { &*db };
    let origin_str = unsafe { CStr::from_ptr(origin) }.to_string_lossy().into_owned();
    let key_str = unsafe { CStr::from_ptr(key) }.to_string_lossy().into_owned();

    let origin_hash = calculate_hash(&origin_str);
    let primary_key = match bincode::serialize(&(origin_hash, &key_str)) {
        Ok(k) => k,
        Err(_) => return -1,
    };

    match &*db.default_container.switcher.current_mode.read() {
        crate::core::modes::CurrentMode::Persistent(pm) => {
            if pm.localstore.increment(primary_key, delta).is_ok() { 0 } else { -1 }
        }
        crate::core::modes::CurrentMode::Ultra(um) => {
            if um.localstore.increment(primary_key, delta).is_ok() { 0 } else { -1 }
        }
    }
}

pub(crate) fn calculate_hash(s: &str) -> u128 {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    let mut s1 = DefaultHasher::new();
    s.hash(&mut s1);
    let mut s2 = DefaultHasher::new();
    "salt".hash(&mut s2);
    s.hash(&mut s2);
    ((s1.finish() as u128) << 64) | (s2.finish() as u128)
}
