#![cfg_attr(not(feature = "std"), no_std)]
#![allow(non_upper_case_globals)]
#![allow(non_camel_case_types)]
#![allow(non_snake_case)]
#![allow(clippy::all)]

#![doc = include_str!("../README.md")]

#[cfg(feature = "alloc")]
extern crate alloc;

pub mod error;
pub mod kvdb;
// pub mod time;
pub mod tsdb;
pub mod utils;

use core::ffi::c_void;

use embedded_storage::nor_flash::NorFlash;

#[cfg(feature = "std")]
pub mod storage;
#[cfg(feature = "std")]
pub use storage::StdStorage;

pub use error::*;

pub use kvdb::*;
pub use tsdb::*;
pub use utils::*;

include!(concat!(env!("OUT_DIR"), "/bindings.rs"));

// 一个不安全的包装器，用于允许在 static 变量中使用包含裸指针的类型
// C 库的 fdb_default_kv 结构体使用了裸指针，但我们知道在 static 上下文中它是只读且安全的
pub struct SyncWrapper<T>(pub T);
unsafe impl<T> Sync for SyncWrapper<T> {}

/// 在编译时定义一组默认的键值对。
///
/// 这个宏会生成一个 `static` 的 `fdb_default_kv` 结构体，
/// 可以被传递给 `KVDB::init` 方法。
///
/// # 示例
///
/// ```
/// use flashdb_rs::define_default_kvs;
/// define_default_kvs! {
///     // 宏的名称将作为生成的 static 变量名
///     MY_DEFAULT_KVS,
///     // 键值对列表
///     "version" => b"1.0.0",
///     "boot_count" => b"\x00\x00\x00\x00", // 值可以是任意字节数组
/// }
///
/// // 稍后在代码中使用
/// // db.init("my_db", Some(&MY_DEFAULT_KVS))?;
/// ```
#[macro_export]
macro_rules! define_default_kvs {
    ($name:ident, $($key:expr => $value:expr),* $(,)?) => {
        static KVS_ARRAY: &[$crate::SyncWrapper<$crate::fdb_default_kv_node>] = &[
            $(
                $crate::SyncWrapper($crate::fdb_default_kv_node {
                    key: concat!($key, "\0").as_ptr() as *mut _,
                    value: $value.as_ptr() as *mut _,
                    value_len: $value.len(),
                }),
            )*
        ];

        #[allow(non_upper_case_globals)]
        pub static $name: $crate::SyncWrapper<$crate::fdb_default_kv> = $crate::SyncWrapper($crate::fdb_default_kv {
            kvs: KVS_ARRAY.as_ptr() as *mut $crate::fdb_default_kv_node,
            num: KVS_ARRAY.len(),
        });
    };
}

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

#[doc(hidden)]
#[repr(C)]
pub struct FlashVTable {
    pub read:
        unsafe extern "C" fn(storage: *mut c_void, addr: u32, buf: *mut u8, size: usize) -> i32,
    pub write:
        unsafe extern "C" fn(storage: *mut c_void, addr: u32, buf: *const u8, size: usize) -> i32,
    pub erase: unsafe extern "C" fn(storage: *mut c_void, addr: u32, size: usize) -> i32,
}

// 调度器结构体
#[doc(hidden)]
#[repr(C)]
pub struct FlashDispatch {
    pub vtable: FlashVTable,
    pub instance: *mut c_void,
}

impl FlashDispatch {
    pub fn new<T: NorFlash>() -> Self {
        return Self {
            vtable: FlashVTable {
                read: vtable_read::<T>,
                write: vtable_write::<T>,
                erase: vtable_erase::<T>,
            },
            instance: core::ptr::null_mut(),
        };
    }
}

// --- VTable 的具体实现函数  ---
unsafe extern "C" fn vtable_read<F: NorFlash>(
    storage: *mut c_void,
    addr: u32,
    buf: *mut u8,
    size: usize,
) -> i32 {
    let flash = &mut *(storage as *mut F);
    let slice = core::slice::from_raw_parts_mut(buf, size);
    match flash.read(addr, slice) {
        Ok(_) => 0,
        Err(_) => -1,
    }
}

unsafe extern "C" fn vtable_write<F: NorFlash>(
    storage: *mut c_void,
    addr: u32,
    buf: *const u8,
    size: usize,
) -> i32 {
    let flash = &mut *(storage as *mut F);
    let slice = core::slice::from_raw_parts(buf, size);
    match flash.write(addr, slice) {
        Ok(_) => 0,
        Err(_) => -1,
    }
}

unsafe extern "C" fn vtable_erase<F: NorFlash>(
    storage: *mut c_void,
    addr: u32,
    size: usize,
) -> i32 {
    let flash = &mut *(storage as *mut F);
    match flash.erase(addr, addr + size as u32) {
        Ok(_) => 0,
        Err(_) => -1,
    }
}

#[no_mangle]
pub unsafe extern "C" fn fdb_custom_read(
    db: fdb_db_t,
    addr: u32,
    buf: *mut c_void,
    size: usize,
) -> fdb_err_t {
    let dispatch = &*((*db).user_data as *const FlashDispatch);
    let result = (dispatch.vtable.read)(dispatch.instance, addr, buf as *mut u8, size);
    if result == 0 {
        crate::fdb_err_t_FDB_NO_ERR
    } else {
        crate::fdb_err_t_FDB_READ_ERR
    }
}

#[no_mangle]
pub unsafe extern "C" fn fdb_custom_write(
    db: fdb_db_t,
    addr: u32,
    buf: *const c_void,
    size: usize,
    _sync: bool,
) -> fdb_err_t {
    let dispatch = &*((*db).user_data as *const FlashDispatch);
    let result = (dispatch.vtable.write)(dispatch.instance, addr, buf as *const u8, size);
    if result == 0 {
        crate::fdb_err_t_FDB_NO_ERR
    } else {
        crate::fdb_err_t_FDB_WRITE_ERR
    }
}

#[no_mangle]
pub unsafe extern "C" fn fdb_custom_erase(db: fdb_db_t, addr: u32, size: usize) -> fdb_err_t {
    let dispatch = &*((*db).user_data as *const FlashDispatch);
    let result = (dispatch.vtable.erase)(dispatch.instance, addr, size);
    if result == 0 {
        crate::fdb_err_t_FDB_NO_ERR
    } else {
        crate::fdb_err_t_FDB_ERASE_ERR
    }
}
