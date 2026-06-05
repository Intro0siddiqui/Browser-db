use std::ffi::{CString, CStr};
use tempfile::tempdir;
use browserdb::ffi::*;

#[test]
fn test_ffi_lifecycle() {
    let dir = tempdir().unwrap();
    let path = CString::new(dir.path().to_str().unwrap()).unwrap();

    let db = browserdb_open(path.as_ptr());
    assert!(!db.is_null());

    let url = CString::new("https://example.com").unwrap();
    let title = CString::new("Example Title").unwrap();

    let res = browserdb_history_insert(db, url.as_ptr(), title.as_ptr(), 1);
    assert_eq!(res, 0);

    // Manual hash calculation for the test (matches ffi.rs)
    let url_str = "https://example.com";
    let url_hash = {
        let mut hash: u128 = 0x6c62272e07bb014262b821756295c58d;
        for byte in url_str.bytes() {
            hash = hash ^ (byte as u128);
            hash = hash.wrapping_mul(0x1000000000000000000013B);
        }
        hash
    };

    let title_ptr = browserdb_history_get_title(db, (url_hash & 0xFFFFFFFFFFFFFFFF) as u64, (url_hash >> 64) as u64);
    assert!(!title_ptr.is_null());

    let title_c = unsafe { CStr::from_ptr(title_ptr) };
    assert_eq!(title_c.to_str().unwrap(), "Example Title");

    browserdb_free_string(title_ptr);
    browserdb_close(db);
}
