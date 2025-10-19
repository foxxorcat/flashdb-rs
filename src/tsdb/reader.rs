use embedded_storage::nor_flash::NorFlash;

use crate::{Error, TSLEntry};

use super::{TSDB,fdb_blob_make_by_tsl};

pub struct TSDBReader<'a,S:NorFlash> {
    position: usize,
    inner: &'a mut TSDB<S>, // 使用原始指针
    pub entry: TSLEntry,
}

impl<'a, S: NorFlash> TSDBReader<'a, S> {
    pub fn new(tsdb: &'a mut TSDB<S>, entry: TSLEntry) -> Self {
        return Self {
            inner: tsdb,
            entry: entry,
            position: 0,
        };
    }
}


impl<'a,S:NorFlash> embedded_io::ErrorType for TSDBReader<'a,S> {
    type Error = Error;
}

impl<'a,S:NorFlash> embedded_io::Read for TSDBReader<'a,S> {
    fn read(&mut self, buf: &mut [u8]) -> Result<usize, Self::Error> {
        let total_len = self.entry.inner.log_len as usize;
        if self.position >= total_len {
            return Ok(0); // EOF
        }

        // 安全：指针生命周期由迭代器保证
        let mut blob = fdb_blob_make_by_tsl(buf, &self.entry, self.position);
        let actual_read = self.inner.fdb_blob_read(&mut blob);
        self.position += actual_read;
        Ok(actual_read)
    }
}

impl<'a,S:NorFlash> embedded_io::Seek for TSDBReader<'a,S> {
    fn seek(&mut self, pos: embedded_io::SeekFrom) -> Result<u64, Self::Error> {
        let total_len = self.entry.inner.log_len as usize;
        let new_pos = match pos {
            embedded_io::SeekFrom::Start(offset) => offset as usize,
            embedded_io::SeekFrom::End(offset) => (total_len as i64 + offset) as usize,
            embedded_io::SeekFrom::Current(offset) => (self.position as i64 + offset) as usize,
        };

        if new_pos > total_len {
            return Err(Error::InvalidArgument);
        }

        self.position = new_pos;
        Ok(new_pos as u64)
    }
}
