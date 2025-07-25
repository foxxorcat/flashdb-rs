use crate::fdb_kv_status;
use crate::{
    error::Error, fdb_blob, fdb_blob_make_by, fdb_blob_make_write, fdb_blob_read, fdb_kv,
    fdb_kv_del, fdb_kv_get_obj, fdb_kv_iterate, fdb_kv_iterator, fdb_kv_iterator_init,
    fdb_kv_set_blob, fdb_kv_set_default, fdb_kv_status_FDB_KV_DELETED,
    fdb_kv_status_FDB_KV_ERR_HDR, fdb_kv_status_FDB_KV_PRE_DELETE, fdb_kv_status_FDB_KV_PRE_WRITE,
    fdb_kv_status_FDB_KV_UNUSED, fdb_kv_status_FDB_KV_WRITE, fdb_kvdb, fdb_kvdb_control_read,
    fdb_kvdb_control_write, fdb_kvdb_deinit, fdb_kvdb_init, fdb_kvdb_t, RawHandle,
    FDB_KVDB_CTRL_SET_FILE_MODE, FDB_KVDB_CTRL_SET_MAX_SIZE, FDB_KVDB_CTRL_SET_NOT_FORMAT,
    FDB_KVDB_CTRL_SET_SEC_SIZE,
};

#[repr(u32)]
#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum KVStatus {
    UNUSED = fdb_kv_status_FDB_KV_UNUSED,
    PRE_WRITE = fdb_kv_status_FDB_KV_PRE_WRITE,
    Write = fdb_kv_status_FDB_KV_WRITE,
    PRE_DELETE = fdb_kv_status_FDB_KV_PRE_DELETE,
    DELETED = fdb_kv_status_FDB_KV_DELETED,
    ERR_HDR = fdb_kv_status_FDB_KV_ERR_HDR,
}
impl From<fdb_kv_status> for KVStatus {
    fn from(value: fdb_kv_status) -> Self {
        unsafe { core::mem::transmute(value) }
    }
}

use alloc::ffi::CString;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

/// KVDB构造器
#[derive(Debug, Clone)]
pub struct KVDBBuilder {
    name: String,
    path: String,
    sec_size: u32, // 扇区大小
    // file_mode: Option<u32>,        // 文件模式
    max_size: Option<u32>, // 最大文件大小（文件模式下使用）
    not_format: bool,      // 初始化时是否不格式化
}
impl KVDBBuilder {
    pub fn file<S: ToString>(name: S, path: S, max_size: u32) -> Self {
        Self {
            name: name.to_string(),
            path: path.to_string(),
            sec_size: 4096,
            max_size: Some(max_size),
            not_format: false,
        }
    }
    /// 设置扇区大小（仅创建模式有效）
    pub fn with_sec_size(mut self, size: u32) -> Self {
        self.sec_size = size;
        self
    }
    /// 设置最大文件大小（文件模式下，仅创建模式有效）
    pub fn  with_max_size(mut self, size: u32) -> Self {
        self.max_size = Some(size);
        self
    }
    /// 设置初始化时不格式化（仅创建模式有效）
    pub fn  with_not_format(mut self, enable: bool) -> Self {
        self.not_format = enable;
        self
    }

    pub fn open(self) -> Result<KVDB, Error> {
        let kvdb = KVDB {
            inner: Default::default(),
        };

        // c 会接管字符串释放, 这里转移所有权
        let name = CString::new(self.name)?.into_raw();
        let path = CString::new(self.path)?.into_raw();

        fdb_kvdb_control_write(kvdb.handle(), FDB_KVDB_CTRL_SET_SEC_SIZE, self.sec_size);
        fdb_kvdb_control_write(kvdb.handle(), FDB_KVDB_CTRL_SET_NOT_FORMAT, self.not_format);

        fdb_kvdb_control_write(kvdb.handle(), FDB_KVDB_CTRL_SET_FILE_MODE, true);
        if let Some(max_size) = self.max_size {
            fdb_kvdb_control_write(kvdb.handle(), FDB_KVDB_CTRL_SET_MAX_SIZE, max_size);
        }

        Error::check_and_return(
            unsafe {
                fdb_kvdb_init(
                    kvdb.handle(),
                    name,
                    path,
                    core::ptr::null_mut(),
                    core::ptr::null_mut(),
                )
            },
            kvdb,
        )
    }
}

pub struct KVDB {
    inner: fdb_kvdb,
}

impl KVDB {
    pub fn set<K: AsRef<str>>(&mut self, key: K, value: &[u8]) -> Result<(), Error> {
        let mut blob = fdb_blob_make_write(value);
        self.fdb_blob_write(key, &mut blob)
    }

    ///
    pub fn get<K: AsRef<str>>(&mut self, key: K) -> Result<Option<Vec<u8>>, Error> {
        match self.fdb_kv_get_obj(key)? {
            Some(kv) => match kv.status.into() {
                // 预写入或已写入
                KVStatus::PRE_WRITE | KVStatus::Write => {
                    let mut data: Vec<u8> = Vec::with_capacity(kv.value_len as _);
                    unsafe { data.set_len(kv.value_len as _) };

                    let mut blob = fdb_blob_make_by(&mut data, &kv, 0);

                    let read_len = self.fdb_blob_read(&mut blob);
                    if read_len != data.len() {
                        return Err(Error::ReadError);
                    }
                    Ok(Some(data))
                }
                _ => Ok(None),
            },
            None => return Ok(None),
        }
    }

    /// 删除 KV
    ///
    /// 在 KVDB 内部实现中，删除 KV 并不会完全从 KVDB 中移除，而是标记为了删除状态，所以删除后数据库剩余容量不会有变化
    pub fn delete<K: AsRef<str>>(&mut self, key: K) -> Result<(), Error> {
        let key = key.as_ref();
        let c_key = CString::new(key)?;
        Error::convert(unsafe { fdb_kv_del(self.handle(), c_key.as_ptr()) })
    }

    /// 重置 KVDB
    ///
    /// 将 KVDB 中的 KV 重置为 首次初始时 的默认值将 KVDB 中的 KV 重置为 首次初始时 的默认值
    pub fn reset(&mut self) -> Result<(), Error> {
        Error::convert(unsafe { fdb_kv_set_default(self.handle()) })
    }

    pub fn get_reader<K: AsRef<str>>(&mut self, key: K) -> Result<KVReader, Error> {
        let key = key.as_ref();
        let c_key = CString::new(key)?;

        let mut kv_obj = unsafe { core::mem::zeroed::<fdb_kv>() };
        if unsafe { fdb_kv_get_obj(self.handle(), c_key.as_ptr(), &mut kv_obj) }
            == core::ptr::null_mut()
        {
            return Err(Error::ReadError);
        };

        return Ok(KVReader {
            inner: self,
            kv_obj: kv_obj,
            position: 0,
            _marker: Default::default(),
        });
    }

    pub fn iter(&mut self) -> KVDBIterator {
        let mut iterator: fdb_kv_iterator = unsafe { core::mem::zeroed() };
        unsafe { fdb_kv_iterator_init(self.handle(), &mut iterator) };
        KVDBIterator {
            inner: self,
            iterator: iterator,
            is_done: false,
        }
    }

    #[inline]
    fn fdb_kv_get_obj<K: AsRef<str>>(&mut self, key: K) -> Result<Option<fdb_kv>, Error> {
        let key = key.as_ref();
        let c_key = CString::new(key)?;
        let mut kv_obj = unsafe { core::mem::zeroed::<fdb_kv>() };

        if unsafe { fdb_kv_get_obj(self.handle(), c_key.as_ptr(), &mut kv_obj) }
            == core::ptr::null_mut()
        {
            return Ok(None);
        };
        Ok(Some(kv_obj))
    }

    #[inline]
    fn fdb_blob_write<K: AsRef<str>>(&mut self, key: K, blob: &mut fdb_blob) -> Result<(), Error> {
        let key = key.as_ref();
        let c_key = CString::new(key)?;
        Error::convert(unsafe { fdb_kv_set_blob(self.handle(), c_key.as_ptr(), blob) })
    }

    #[inline]
    fn fdb_blob_read(&mut self, blob: &mut fdb_blob) -> usize {
        unsafe { fdb_blob_read(self.handle() as *mut _, blob) }
    }

    #[inline]
    fn fdb_kvdb_control_write<T>(&mut self, cmd: u32, arg: T) {
        fdb_kvdb_control_write(self.handle(), cmd, arg)
    }

    #[inline]
    fn fdb_kvdb_control_read<T>(&self, cmd: u32, arg: &mut T) {
        fdb_kvdb_control_read(self.handle(), cmd, arg)
    }
}

impl RawHandle for KVDB {
    type Handle = fdb_kvdb_t;

    fn handle(&self) -> Self::Handle {
        &self.inner as *const _ as *mut _
    }
}

impl Drop for KVDB {
    fn drop(&mut self) {
        unsafe {
            fdb_kvdb_deinit(self.handle());
        }
    }
}

unsafe impl Send for KVDB {}
// unsafe impl Sync for KVDB {}

/// KVDB 迭代器
pub struct KVDBIterator<'a> {
    inner: &'a mut KVDB,
    iterator: fdb_kv_iterator,
    is_done: bool,
}

pub struct KVEntry<'a> {
    pub key: String,
    pub reader: KVReader<'a>,
}

impl<'a> Iterator for KVDBIterator<'a> {
    type Item = Result<KVEntry<'a>, Error>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.is_done {
            return None;
        }

        if unsafe { fdb_kv_iterate(self.inner.handle(), &mut self.iterator) } == false {
            self.is_done = true;
            return None;
        }

        let curr_kv = self.iterator.curr_kv.clone();
        let key_ptr = curr_kv.name.as_ptr() as *const u8;
        let key_len = curr_kv.name_len as usize;

        let key = match unsafe {
            let c_str_slice = core::slice::from_raw_parts(key_ptr, key_len);
            core::str::from_utf8(c_str_slice)
        } {
            Ok(key) => key.to_string(),
            Err(_) => return Some(Err(Error::KvNameError)),
        };

        let inner_ptr = self.inner as *mut KVDB;

        Some(Ok(KVEntry {
            key,
            reader: KVReader {
                inner: inner_ptr,
                kv_obj: curr_kv,
                position: 0,
                _marker: core::marker::PhantomData,
            },
        }))
    }
}

pub struct KVReader<'a> {
    inner: *mut KVDB, // 使用原始指针
    kv_obj: fdb_kv,
    position: usize,
    _marker: core::marker::PhantomData<&'a mut KVDB>, // 生命周期标记
}

impl KVReader<'_> {
    pub fn status(&self) -> KVStatus {
        return self.kv_obj.status.into();
    }

    pub fn crc_is_ok(&self) -> bool {
        return self.kv_obj.crc_is_ok;
    }
}

impl<'a> embedded_io::ErrorType for KVReader<'a> {
    type Error = Error;
}

impl<'a> embedded_io::Read for KVReader<'a> {
    fn read(&mut self, buf: &mut [u8]) -> Result<usize, Self::Error> {
        if self.position >= self.kv_obj.value_len as usize {
            return Ok(0); // EOF
        }

        // 安全：指针生命周期由迭代器保证
        let kvdb = unsafe { &mut *self.inner };
        let mut blob = fdb_blob_make_by(buf, &self.kv_obj, self.position);
        let actual_read = kvdb.fdb_blob_read(&mut blob);
        self.position += actual_read;
        Ok(actual_read)
    }
}

impl<'a> embedded_io::Seek for KVReader<'a> {
    fn seek(&mut self, pos: embedded_io::SeekFrom) -> Result<u64, Self::Error> {
        let total_len = self.kv_obj.value_len as usize;
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
