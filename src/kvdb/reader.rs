use crate::{fdb_blob_make_by, fdb_blob_read, Error, RawHandle};
use embedded_storage::nor_flash::NorFlash;

use super::{KVEntry, KVDB};

/// KV值读取器
///
/// 实现了embedded-io的Read和Seek trait，用于流式读取KV值，适合处理大型数据
/// 生命周期`'a`确保读取器不会超过其关联的KVDB实例的生命周期
pub struct KVReader<'a, S: NorFlash> {
    position: usize,        // 当前读取位置
    inner: &'a mut KVDB<S>, // 指向KVDB实例的指针
    pub entry: KVEntry,     // KV对象元数据
}

impl<'a, S: NorFlash> KVReader<'a, S> {
    pub fn new(kvdb: &'a mut KVDB<S>, entry: KVEntry) -> Self {
        return Self {
            inner: kvdb,
            entry: entry,
            position: 0,
        };
    }
}

impl<'a, S: NorFlash> embedded_io::ErrorType for KVReader<'a, S> {
    type Error = Error;
}

impl<'a, S: NorFlash> embedded_io::Read for KVReader<'a, S> {
    /// 从KV值中读取数据到缓冲区
    ///
    /// # 参数
    /// - `buf`: 接收数据的缓冲区
    ///
    /// # 返回值
    /// 成功时返回读取的字节数，失败时返回Error
    fn read(&mut self, buf: &mut [u8]) -> Result<usize, Self::Error> {
        if self.position >= self.entry.value_len() {
            return Ok(0); // EOF
        }
        let mut blob = fdb_blob_make_by(buf, &self.entry, self.position);
        let read_len = unsafe { fdb_blob_read(self.inner.handle() as *mut _, &mut blob) };
        self.position += read_len;
        Ok(read_len)
    }
}

impl<'a, S: NorFlash> embedded_io::Seek for KVReader<'a, S> {
    /// 调整读取位置
    ///
    /// # 参数
    /// - `pos`: 要定位的位置，支持从开始、当前位置或末尾偏移
    ///
    /// # 返回值
    /// 成功时返回新的位置，失败时返回Error
    fn seek(&mut self, pos: embedded_io::SeekFrom) -> Result<u64, Self::Error> {
        let total_len = self.entry.value_len();
        // 根据SeekFrom计算新位置
        let new_pos = match pos {
            embedded_io::SeekFrom::Start(offset) => offset as usize,
            embedded_io::SeekFrom::End(offset) => (total_len as i64 + offset) as usize,
            embedded_io::SeekFrom::Current(offset) => (self.position as i64 + offset) as usize,
        };

        // 检查新位置是否有效
        if new_pos > total_len {
            return Err(Error::InvalidArgument);
        }

        self.position = new_pos;
        Ok(new_pos as u64)
    }
}
