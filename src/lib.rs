#![allow(non_upper_case_globals)]
#![allow(non_camel_case_types)]
#![allow(non_snake_case)]
#![allow(clippy::all)]
#![no_std]
extern crate alloc;

pub mod error;
pub mod kvdb;
// pub mod time;
pub mod tsdb;
pub mod utils;

pub use error::*;
pub use utils::*;
pub use kvdb::*;
pub use tsdb::*;

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
