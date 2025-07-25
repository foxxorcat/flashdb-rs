//! # 通用存储层
//!
//! 这个模块定义了 FlashDB 的存储后端抽象。
//!
//! - `Storage`: 一个核心 Trait，定义了所有存储后端必须实现的 I/O 操作。
//! - `HasStorage`: 一个内部 Trait，用于辅助实现泛型回调函数。
//! - `std_impl`: 一个仅在 `std` 特性下可用的子模块，提供了基于文件的 `Storage` 实现。
//! - **FFI 回调**: 提供了 C 库所需的 `extern "C"` 回调函数，它们是泛型的，可以与任何实现了 `HasStorage` 的数据库类型一起工作。

use crate::{error::Error, fdb_err_t};
use core::ffi::c_void;
use embedded_io::{Read, Seek, Write};

// --- 核心 Trait 定义 ---

pub trait Storage: Read<Error = Error> + Write<Error = Error> + Seek<Error = Error> {
    /// 擦除指定地址和大小的区域。
    fn erase(&mut self, addr: u32, size: usize) -> Result<(), Error>;
}

/// C 回调函数，用于处理读操作。
///
/// 它接收的 `user_data` 是一个指向 `Box<dyn Storage>` 的裸指针。
/// 它将指针转回 Rust 的 Trait 对象引用，并调用相应的方法。
pub unsafe extern "C" fn rust_read_callback(
    user_data: *mut c_void,
    addr: u32,
    buf: *mut c_void,
    size: usize,
) -> fdb_err_t {
    let storage = &mut *(user_data as *mut Box<dyn Storage>);

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
pub unsafe extern "C" fn rust_write_callback(
    user_data: *mut c_void,
    addr: u32,
    buf: *const c_void,
    size: usize,
    sync: bool,
) -> fdb_err_t {
    let storage = &mut *(user_data as *mut Box<dyn Storage>);

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
pub unsafe extern "C" fn rust_erase_callback(
    user_data: *mut c_void,
    addr: u32,
    size: usize,
) -> fdb_err_t {
    let storage = &mut *(user_data as *mut Box<dyn Storage>);
    match storage.erase(addr, size) {
        Ok(_) => crate::fdb_err_t_FDB_NO_ERR,
        Err(_) => crate::fdb_err_t_FDB_ERASE_ERR,
    }
}

// --- `std` 环境下的具体实现 ---

/// 仅在 `std` 特性启用时，才编译此模块。
#[cfg(feature = "std")]
pub mod std_impl {
    use super::{Error, Read, Seek, Storage, Write};
    use std::fs::{File, OpenOptions};
    use std::io::prelude::{Read as StdRead, Seek as StdSeek, Write as StdWrite};
    use std::path::Path;
    use std::vec;

    /// 一个基于 `std::fs::File` 的 `Storage` 实现，用于桌面环境。
    pub struct StdStorage {
        file: File,
    }

    impl StdStorage {
        /// 通过文件路径创建一个新的 `StdStorage` 实例。
        pub fn new<P: AsRef<Path>>(path: P) -> Result<Self, std::io::Error> {
            let file = OpenOptions::new()
                .read(true)
                .write(true)
                .create(true)
                .open(path)?;
            Ok(Self { file })
        }
    }

    impl embedded_io::ErrorType for StdStorage {
        type Error = Error;
    }

    impl Read for StdStorage {
        fn read(&mut self, buf: &mut [u8]) -> Result<usize, Self::Error> {
            self.file.read(buf).map_err(|err| {
                #[cfg(feature = "log")]
                log::error!("StdStorage seek: {}", err);
                Error::ReadError
            })
        }
    }

    impl Write for StdStorage {
        fn write(&mut self, buf: &[u8]) -> Result<usize, Self::Error> {
            self.file.write(buf).map_err(|err| {
                #[cfg(feature = "log")]
                log::error!("StdStorage seek: {}", err);
                Error::WriteError
            })
        }
        fn flush(&mut self) -> Result<(), Self::Error> {
            self.file.flush().map_err(|err| {
                #[cfg(feature = "log")]
                log::error!("StdStorage seek: {}", err);
                Error::WriteError
            })
        }
    }

    impl Seek for StdStorage {
        fn seek(&mut self, pos: embedded_io::SeekFrom) -> Result<u64, Self::Error> {
            let whence = match pos {
                embedded_io::SeekFrom::Start(p) => std::io::SeekFrom::Start(p),
                embedded_io::SeekFrom::End(p) => std::io::SeekFrom::End(p),
                embedded_io::SeekFrom::Current(p) => std::io::SeekFrom::Current(p),
            };
            self.file.seek(whence).map_err(|err| {
                #[cfg(feature = "log")]
                log::error!("StdStorage seek: {}", err);
                println!("StdStorage seek: {}", err);
                Error::ReadError
            })
        }
    }

    impl Storage for StdStorage {
        fn erase(&mut self, addr: u32, size: usize) -> Result<(), Error> {
            self.seek(embedded_io::SeekFrom::Start(addr as u64))?;
            let buf = vec![0xFF; size];
            self.write_all(&buf)?;
            self.flush()
        }
    }
}
