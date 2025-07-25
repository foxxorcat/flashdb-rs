#![cfg_attr(not(feature = "std"), no_std)]
#![allow(non_upper_case_globals)]
#![allow(non_camel_case_types)]
#![allow(non_snake_case)]
#![allow(clippy::all)]

extern crate alloc;

pub mod error;
pub mod kvdb;
// pub mod time;
pub mod tsdb;
pub mod utils;

pub mod storage;
use core::ffi::c_void;

use storage::Storage;

#[cfg(feature = "std")]
pub use storage::std_impl::StdStorage;

pub use error::*;

pub use kvdb::*;
pub use tsdb::*;
pub use utils::*;

include!(concat!(env!("OUT_DIR"), "/bindings.rs"));

pub trait RawHandle {
    type Handle;

    /// Care should be taken to use the returned ESP-IDF driver raw handle only while
    /// the driver is still alive, so as to avoid use-after-free errors.
    fn handle(&self) -> Self::Handle;
}

// 暴露给 C 的日志函数
#[no_mangle]
#[cfg(feature = "log")]
pub extern "C" fn rust_log(message: *const core::ffi::c_char) {
    let c_str = unsafe { core::ffi::CStr::from_ptr(message) };
    if let Ok(message_str) = c_str.to_str() {
        // 使用 Rust 的 log 宏输出（日志级别设为 INFO）
        log::log!(log::Level::Info, "{message_str}");
    }
}

/// C 回调函数，用于处理读操作。
///
/// 它接收的 `user_data` 是一个指向 `Box<dyn Storage>` 的裸指针。
/// 它将指针转回 Rust 的 Trait 对象引用，并调用相应的方法。
#[no_mangle]
pub unsafe extern "C" fn fdb_custom_read(
    db: fdb_db_t,
    addr: u32,
    buf: *mut c_void,
    size: usize,
) -> fdb_err_t {
    let storage = &mut *((*db).user_data as *mut Box<dyn Storage>);

    if storage
        .seek(embedded_io::SeekFrom::Start(addr as u64))
        .is_err()
    {
        return crate::fdb_err_t_FDB_READ_ERR;
    }
    let rust_slice = core::slice::from_raw_parts_mut(buf as *mut u8, size);

    match storage.read_exact(rust_slice) {
        Ok(_) => crate::fdb_err_t_FDB_NO_ERR,
        Err(_) => crate::fdb_err_t_FDB_READ_ERR,
    }
}

/// C 回调函数，用于处理写操作。
#[no_mangle]
pub unsafe extern "C" fn fdb_custom_write(
    db: fdb_db_t,
    addr: u32,
    buf: *const c_void,
    size: usize,
    sync: bool,
) -> fdb_err_t {
    let storage = &mut *((*db).user_data as *mut Box<dyn Storage>);

    if storage
        .seek(embedded_io::SeekFrom::Start(addr as u64))
        .is_err()
    {
        return crate::fdb_err_t_FDB_WRITE_ERR;
    }
    let rust_slice = core::slice::from_raw_parts(buf as *const u8, size);
    match storage.write_all(rust_slice) {
        Ok(_) => {
            if sync {
                let _ = storage.flush();
            }
            crate::fdb_err_t_FDB_NO_ERR
        }
        Err(_) => crate::fdb_err_t_FDB_WRITE_ERR,
    }
}

/// C 回调函数，用于处理擦除操作。
#[no_mangle]
pub unsafe extern "C" fn fdb_custom_erase(db: fdb_db_t, addr: u32, size: usize) -> fdb_err_t {
    let storage = &mut *((*db).user_data as *mut Box<dyn Storage>);
    match storage.erase(addr, size) {
        Ok(_) => crate::fdb_err_t_FDB_NO_ERR,
        Err(_) => crate::fdb_err_t_FDB_ERASE_ERR,
    }
}
