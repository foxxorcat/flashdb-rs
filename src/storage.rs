use crate::error::Error;
use embedded_storage::nor_flash::{ErrorType, NorFlash, ReadNorFlash};
use lru::LruCache;
use std::fs::{File, OpenOptions};
use std::io::prelude::{Read as StdRead, Seek as StdSeek, Write as StdWrite};
use std::io::ErrorKind;
use std::num::NonZeroUsize;
use std::path::{Path, PathBuf};

/// 定义文件存储策略
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileStrategy {
    /// **单文件模式**: 整个数据库存储在一个大文件中。
    Single,
    /// **多文件模式**: 每个扇区存储在一个独立的文件中 (例如 `kv_db.fdb.0`, `kv_db.fdb.1`)。
    /// 这兼容原始 C 库的文件模式。
    Multi,
}

/// 一个基于 `std::fs::File` 的 `NorFlash` 实现，用于桌面环境。
///
/// 通过 LRU 缓存高效管理文件句柄。
pub struct StdStorage {
    strategy: FileStrategy,
    db_name: String,
    base_path: PathBuf,
    sec_size: u32,
    capacity: u32,
    file_cache: LruCache<u32, File>,
}

impl StdStorage {
    /// 创建一个新的 `StdStorage` 实例。
    pub fn new<P: AsRef<Path>>(
        path: P,
        db_name: &str,
        sec_size: u32,
        capacity: u32,
        strategy: FileStrategy,
    ) -> Result<Self, std::io::Error> {
        let base_path = path.as_ref().to_path_buf();
        if strategy == FileStrategy::Multi {
            std::fs::create_dir_all(&base_path)?;
        } else {
            // 单文件模式，确保父目录存在
            if let Some(parent) = base_path.parent() {
                std::fs::create_dir_all(parent)?;
            }
        }
        Ok(Self {
            strategy,
            db_name: db_name.to_string(),
            sec_size,
            capacity,
            base_path,
            file_cache: LruCache::new(NonZeroUsize::new(8).unwrap()),
        })
    }

    /// 根据地址获取对应的文件句柄和文件内偏移量。
    fn get_file_and_offset(&mut self, addr: u32) -> Result<(&mut File, u64), std::io::Error> {
        let (sector_index, offset_in_file) = match self.strategy {
            FileStrategy::Single => (0, addr as u64),
            FileStrategy::Multi => {
                let sector_index = addr / self.sec_size;
                let offset = (addr % self.sec_size) as u64;
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

impl ErrorType for StdStorage {
    type Error = Error;
}

impl ReadNorFlash for StdStorage {
    const READ_SIZE: usize = 1;

    fn read(&mut self, offset: u32, bytes: &mut [u8]) -> Result<(), Self::Error> {
        let (file, file_offset) = self.get_file_and_offset(offset)?;
        file.seek(std::io::SeekFrom::Start(file_offset))?;
        match file.read_exact(bytes) {
            Ok(_) => Ok(()),
            Err(e) if e.kind() == ErrorKind::UnexpectedEof => {
                // Flash 存储在未写入区域读取时通常返回0xFF
                bytes.fill(0xFF);
                Ok(())
            }
            Err(e) => Err(e.into()),
        }
    }

    fn capacity(&self) -> usize {
        self.capacity as usize
    }
}

impl NorFlash for StdStorage {
    const WRITE_SIZE: usize = 1;
    const ERASE_SIZE: usize = 4096; // 这是一个典型值，我们将 sec_size 作为擦除大小

    fn erase(&mut self, from: u32, to: u32) -> Result<(), Self::Error> {
        let size = to - from;
        // 擦除操作是基于绝对地址的
        let (sector_index, offset) = match self.strategy {
            FileStrategy::Single => (0, from as u64),
            FileStrategy::Multi => {
                if from % self.sec_size != 0 || size != self.sec_size {
                    return Err(Error::InvalidArgument);
                }
                (from / self.sec_size, 0)
            }
        };

        self.file_cache.pop(&sector_index);
        let file_path = match self.strategy {
            FileStrategy::Single => self.base_path.clone(),
            FileStrategy::Multi => self
                .base_path
                .join(format!("{}.fdb.{}", self.db_name, sector_index)),
        };

        let mut file = OpenOptions::new().write(true).create(true).open(&file_path)?;

        if self.strategy == FileStrategy::Multi {
            file.set_len(0)?;
        }

        file.seek(std::io::SeekFrom::Start(offset))?;
        // 模拟擦除，填充 0xFF
        let buf = vec![0xFF; size as usize];
        file.write_all(&buf)?;
        file.flush()?;
        Ok(())
    }

    fn write(&mut self, offset: u32, bytes: &[u8]) -> Result<(), Self::Error> {
        let (file, file_offset) = self.get_file_and_offset(offset)?;
        file.seek(std::io::SeekFrom::Start(file_offset))?;
        file.write_all(bytes)?;
        file.flush()?;
        Ok(())
    }
}