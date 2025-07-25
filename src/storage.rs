//! # 通用存储层
//!
//! 这个模块定义了 FlashDB 的存储后端抽象。
//!
//! - `Storage`: 一个核心 Trait，定义了所有存储后端必须实现的 I/O 操作。
//! - `HasStorage`: 一个内部 Trait，用于辅助实现泛型回调函数。
//! - `std_impl`: 一个仅在 `std` 特性下可用的子模块，提供了基于文件的 `Storage` 实现。
//! - **FFI 回调**: 提供了 C 库所需的 `extern "C"` 回调函数，它们是泛型的，可以与任何实现了 `HasStorage` 的数据库类型一起工作。

use crate::error::Error;
use embedded_io::{Read, Seek, Write};

pub trait Storage: Read<Error = Error> + Write<Error = Error> + Seek<Error = Error> {
    /// 擦除指定地址和大小的区域。
    fn erase(&mut self, addr: u32, size: usize) -> Result<(), Error>;
}

/// 仅在 `std` 特性启用时，才编译此模块。
#[cfg(feature = "std")]
pub mod std_impl {
    use super::{Error, Read, Seek, Storage, Write};
    use lru::LruCache;
    use std::fs::{File, OpenOptions};
    use std::io::prelude::{Read as StdRead, Seek as StdSeek, Write as StdWrite};
    use std::num::NonZeroUsize;
    use std::path::{Path, PathBuf};
    use std::vec;

    /// 定义文件存储策略
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum FileStrategy {
        /// **单文件模式**: 整个数据库存储在一个文件中。性能更高。
        Single,
        /// **多文件模式**: 每个扇区存储在一个独立的文件中 (例如 `kv_db.fdb.0`, `kv_db.fdb.1`)。
        /// 用于兼容原始 C 库的文件模式。
        Multi,
    }

    /// 一个基于 `std::fs::File` 的 `Storage` 实现，用于桌面环境。
    ///
    /// 它通过 `FileStrategy` 支持两种模式，并通过 LRU 缓存高效管理文件句柄。
    pub struct StdStorage {
        strategy: FileStrategy,
        db_name: String,
        sec_size: u32,
        base_path: PathBuf, // 在单文件模式下是完整文件路径，在多文件模式下是目录路径
        // 文件句柄缓存。键是扇区索引，值是文件句柄。
        file_cache: LruCache<u32, File>,
        // 当前的逻辑光标位置，由 `seek` 更新。
        position: u64,
    }

    impl StdStorage {
        /// 创建一个新的 `StdStorage` 实例。
        ///
        /// # 参数
        /// - `path`: 文件或目录的路径。
        /// - `db_name`: 数据库名称，用于在多文件模式下构造文件名。
        /// - `sec_size`: 扇区大小，用于在多文件模式下计算文件索引。
        /// - `strategy`: 文件存储策略 (`Single` 或 `Multi`)。
        pub fn new<P: AsRef<Path>>(
            path: P,
            db_name: &str,
            sec_size: u32,
            strategy: FileStrategy,
        ) -> Result<Self, std::io::Error> {
            let base_path = path.as_ref().to_path_buf();
            if strategy == FileStrategy::Multi {
                // 在多文件模式下，确保基础路径是一个目录
                std::fs::create_dir_all(&base_path)?;
            }
            Ok(Self {
                strategy,
                db_name: db_name.to_string(),
                sec_size,
                base_path,
                // 缓存8个最近使用的文件句柄
                file_cache: LruCache::new(NonZeroUsize::new(8).unwrap()),
                position: 0,
            })
        }

        /// 内部辅助函数：根据当前 `position` 获取对应的文件句柄和文件内偏移量。
        fn get_file_and_offset(&mut self) -> Result<(&mut File, u64), Error> {
            let (sector_index, offset_in_file) = match self.strategy {
                FileStrategy::Single => (0, self.position),
                FileStrategy::Multi => {
                    let sector_index = (self.position / self.sec_size as u64) as u32;
                    let offset = self.position % self.sec_size as u64;
                    (sector_index, offset)
                }
            };

            if !self.file_cache.contains(&sector_index) {
                let file_path = match self.strategy {
                    FileStrategy::Single => self.base_path.clone(),
                    FileStrategy::Multi => self
                        .base_path
                        .join(format!("{}.fdb.{}", self.db_name, sector_index)),
                };
                let file = OpenOptions::new()
                    .read(true)
                    .write(true)
                    .create(true)
                    .open(&file_path)?;
                self.file_cache.put(sector_index, file);
            }

            let file = self.file_cache.get_mut(&sector_index).unwrap();
            Ok((file, offset_in_file))
        }
    }

    impl embedded_io::ErrorType for StdStorage {
        type Error = Error;
    }

    impl Read for StdStorage {
        fn read(&mut self, buf: &mut [u8]) -> Result<usize, Self::Error> {
            let (file, offset) = self.get_file_and_offset()?;
            file.seek(std::io::SeekFrom::Start(offset))?;
            let bytes_read = file.read(buf)?;
            self.position += bytes_read as u64;
            Ok(bytes_read)
        }
    }

    impl Write for StdStorage {
        fn write(&mut self, buf: &[u8]) -> Result<usize, Self::Error> {
            let (file, offset) = self.get_file_and_offset()?;
            file.seek(std::io::SeekFrom::Start(offset))?;
            let bytes_written = file.write(buf)?;
            self.position += bytes_written as u64;
            Ok(bytes_written)
        }

        fn flush(&mut self) -> Result<(), Self::Error> {
            // 刷新缓存中所有的文件句柄
            for (_, file) in self.file_cache.iter_mut() {
                file.flush()?;
            }
            Ok(())
        }
    }

    impl Seek for StdStorage {
        fn seek(&mut self, pos: embedded_io::SeekFrom) -> Result<u64, Self::Error> {
            // Seek 只是更新逻辑光标位置，实际的文件指针移动在 Read/Write 中完成。
            // 这里的 total_len 是一个逻辑值，我们用一个较大的数来近似，因为实际大小是动态的。
            let total_len = u64::MAX;
            let new_pos = match pos {
                embedded_io::SeekFrom::Start(p) => p,
                embedded_io::SeekFrom::End(p) => (total_len as i64 + p) as u64,
                embedded_io::SeekFrom::Current(p) => (self.position as i64 + p) as u64,
            };
            self.position = new_pos;
            Ok(self.position)
        }
    }

    impl Storage for StdStorage {
        fn erase(&mut self, addr: u32, size: usize) -> Result<(), Error> {
            // 擦除操作是基于绝对地址的，不使用内部的 position。
            let (sector_index, offset) = match self.strategy {
                FileStrategy::Single => (0, addr as u64),
                FileStrategy::Multi => {
                    // 擦除必须在扇区边界上
                    if addr % self.sec_size != 0 || size as u32 != self.sec_size {
                        return Err(Error::InvalidArgument);
                    }
                    ((addr / self.sec_size) as u32, 0)
                }
            };

            // 如果文件在缓存中，先移除，因为我们将要修改它
            self.file_cache.pop(&sector_index);

            let file_path = match self.strategy {
                FileStrategy::Single => self.base_path.clone(),
                FileStrategy::Multi => self
                    .base_path
                    .join(format!("{}.fdb.{}", self.db_name, sector_index)),
            };

            let mut file = OpenOptions::new()
                .write(true)
                .create(true)
                .open(&file_path)?;

            if self.strategy == FileStrategy::Multi {
                // 多文件模式下，截断文件以清空
                file.set_len(0)?;
            }

            file.seek(std::io::SeekFrom::Start(offset))?;
            let buf = vec![0xFF; size];
            file.write_all(&buf)?;
            file.flush()?;
            Ok(())
        }
    }
}
