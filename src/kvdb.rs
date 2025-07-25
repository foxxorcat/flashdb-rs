//! # 键值数据库 (KVDB) 模块
//!
//! 该模块提供了一个键值数据库(KVDB)的Rust封装，基于底层C库实现，支持自定义存储后端。
//! 主要功能包括键值对的增删改查、迭代遍历、状态管理等，同时支持文件存储和自定义存储两种模式。

use crate::storage::std_impl::FileStrategy;
use crate::{
    error::Error, fdb_blob, fdb_blob_make_by, fdb_blob_make_write, fdb_blob_read, fdb_kv,
    fdb_kv_del, fdb_kv_get_obj, fdb_kv_iterate, fdb_kv_iterator, fdb_kv_iterator_init,
    fdb_kv_set_blob, fdb_kv_set_default, fdb_kv_status, fdb_kv_status_FDB_KV_DELETED,
    fdb_kv_status_FDB_KV_ERR_HDR, fdb_kv_status_FDB_KV_PRE_DELETE, fdb_kv_status_FDB_KV_PRE_WRITE,
    fdb_kv_status_FDB_KV_UNUSED, fdb_kv_status_FDB_KV_WRITE, fdb_kvdb, fdb_kvdb_deinit,
    fdb_kvdb_init, fdb_kvdb_t, RawHandle, Storage,
};
use crate::{
    fdb_db_t, fdb_kvdb_control_read, fdb_kvdb_control_write, FDB_KVDB_CTRL_SET_MAX_SIZE,
    FDB_KVDB_CTRL_SET_NOT_FORMAT, FDB_KVDB_CTRL_SET_SEC_SIZE,
};
use alloc::ffi::CString;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use core::marker::PhantomData;
use std::path::Path;

/// 键值对状态枚举
///
/// 表示KV在数据库中的生命周期状态，用于跟踪键值对的操作历史和当前状态
#[repr(u32)]
#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum KVStatus {
    /// 未使用状态，初始创建但未被操作
    UNUSED = fdb_kv_status_FDB_KV_UNUSED,
    /// 预写入状态，准备写入但尚未完成
    PRE_WRITE = fdb_kv_status_FDB_KV_PRE_WRITE,
    /// 已写入状态，数据已成功写入
    Write = fdb_kv_status_FDB_KV_WRITE,
    /// 预删除状态，准备删除但尚未完成
    PRE_DELETE = fdb_kv_status_FDB_KV_PRE_DELETE,
    /// 已删除状态，数据已标记为删除
    DELETED = fdb_kv_status_FDB_KV_DELETED,
    /// 头部错误状态，键值对头部信息损坏
    ERR_HDR = fdb_kv_status_FDB_KV_ERR_HDR,
}

impl From<fdb_kv_status> for KVStatus {
    /// 从底层C类型转换为Rust枚举类型
    ///
    /// # 安全性
    /// 依赖于C库定义的状态值与Rust枚举变体的数值匹配
    fn from(value: fdb_kv_status) -> Self {
        unsafe { core::mem::transmute(value) }
    }
}

/// KVDB构建器
///
/// 用于配置和创建KVDB实例，支持设置数据库名称、路径、扇区大小等参数
#[derive(Debug, Clone)]
pub struct KVDBBuilder {
    name: String,         // 数据库名称
    path: Option<String>, // 数据库存储路径(可选)
    sec_size: u32,        // 扇区大小
    max_size: u32,        // 数据库最大容量
    not_format: bool,     // 是否禁止格式化
}

impl KVDBBuilder {
    /// 创建新的KVDB构建器
    ///
    /// # 参数
    /// - `name`: 数据库名称
    /// - `max_size`: 数据库最大容量(字节)
    pub fn new(name: &str, max_size: u32) -> Self {
        Self {
            name: name.to_string(),
            path: None,
            sec_size: 4096, // 默认扇区大小4096字节
            max_size,
            not_format: false,
        }
    }

    /// 设置扇区大小
    ///
    /// # 参数
    /// - `size`: 扇区大小(字节)
    pub fn with_sec_size(mut self, size: u32) -> Self {
        self.sec_size = size;
        self
    }

    /// 设置是否禁止格式化
    ///
    /// # 参数
    /// - `enable`: 为true时禁止格式化数据库
    pub fn with_not_format(mut self, enable: bool) -> Self {
        self.not_format = enable;
        self
    }

    /// 使用指定的存储后端打开数据库
    ///
    /// # 参数
    /// - `storage`: 实现Storage trait的存储后端实例
    ///
    /// # 返回值
    /// 成功时返回KVDB实例，失败时返回Error
    pub fn open_with<S: Storage + 'static>(self, storage: S) -> Result<KVDB, Error> {
        let storage_boxed = Box::new(Box::new(storage) as Box<dyn Storage>);
        let storage_boxed_raw = Box::into_raw(storage_boxed);
        let storage = unsafe { Box::from_raw(storage_boxed_raw) };

        let name = CString::new(self.name).unwrap();
        let path = CString::new(self.path.unwrap_or_default()).unwrap();

        let mut kvdb = KVDB {
            name,
            path,
            inner: Default::default(), // 初始化内部C结构体
            storage,                   // 存储后端实例
        };

        unsafe {
            // 获取数据库指针并配置存储回调
            let db_ptr = kvdb.handle() as fdb_db_t;

            (*db_ptr).mode = crate::fdb_storage_type_FDB_STORAGE_CUSTOM;

            // 设置数据库参数
            kvdb.fdb_kvdb_control_write(FDB_KVDB_CTRL_SET_SEC_SIZE, self.sec_size);
            kvdb.fdb_kvdb_control_write(FDB_KVDB_CTRL_SET_MAX_SIZE, self.max_size);
            kvdb.fdb_kvdb_control_write(FDB_KVDB_CTRL_SET_NOT_FORMAT, self.not_format);

            // 初始化数据库
            let result = fdb_kvdb_init(
                db_ptr as *mut fdb_kvdb,
                kvdb.name.as_ptr(),
                kvdb.path.as_ptr(),
                core::ptr::null_mut(),
                storage_boxed_raw as *mut _,
            );

            Error::check_and_return(result, kvdb)
        }
    }
}

#[cfg(feature = "std")]
impl KVDBBuilder {
    /// 创建基于文件的KVDB构建器
    ///
    /// # 参数
    /// - `name`: 数据库名称
    /// - `path`: 数据库文件存储路径
    /// - `max_size`: 数据库最大容量
    pub fn file(name: &str, path: &str, max_size: u32) -> Self {
        Self {
            name: name.to_string(),
            path: Some(path.to_string()),
            sec_size: 4096,
            max_size,
            not_format: false,
        }
    }

    /// 链式设置存储路径
    pub fn with_path<S: ToString>(mut self, path: S) -> Self {
        self.path = Some(path.to_string());
        self
    }

    /// 打开基于标准文件系统的KVDB
    ///
    /// # 返回值
    /// 成功时返回KVDB实例，失败时返回Error
    pub fn open(self) -> Result<KVDB, Error> {
        // 获取存储路径
        let path = self.path.clone().ok_or(Error::InvalidArgument)?;

        // 创建标准存储后端
        let storage = crate::StdStorage::new(path, &self.name, self.sec_size, FileStrategy::Multi)?;
        self.open_with(storage)
    }
}

/// 键值数据库(KVDB)核心结构体
///
/// 封装了底层键值数据库实现，提供键值对的操作接口
pub struct KVDB {
    name: CString,
    path: CString,
    inner: fdb_kvdb,                // 底层C库的数据库结构体
    storage: Box<Box<dyn Storage>>, // 存储后端实例
}

impl KVDB {
    /// 向数据库中设置键值对
    ///
    /// # 参数
    /// - `key`: 键(字符串)
    /// - `value`: 值(字节数组)
    ///
    /// # 返回值
    /// 成功时返回Ok(())，失败时返回Error
    pub fn set<K: AsRef<str>>(&mut self, key: K, value: &[u8]) -> Result<(), Error> {
        let mut blob = fdb_blob_make_write(value); // 创建写入用的blob结构
        self.fdb_blob_write(key, &mut blob)
    }

    /// 从数据库中获取键对应的值
    ///
    /// # 参数
    /// - `key`: 要查询的键
    ///
    /// # 返回值
    /// 成功时返回Some(字节数组)或None(键不存在)，失败时返回Error
    pub fn get<K: AsRef<str>>(&mut self, key: K) -> Result<Option<Vec<u8>>, Error> {
        match self.fdb_kv_get_obj(key)? {
            Some(kv) => match kv.status.into() {
                // 处理预写入或已写入状态的值
                KVStatus::PRE_WRITE | KVStatus::Write => {
                    // 初始化缓冲区
                    let mut data: Vec<u8> = Vec::with_capacity(kv.value_len as _);
                    unsafe { data.set_len(kv.value_len as _) }; // 预分配缓冲区大小

                    // 创建读取用的blob结构
                    let mut blob = fdb_blob_make_by(&mut data, &kv, 0);

                    // 读取数据
                    let read_len = self.fdb_blob_read(&mut blob);
                    if read_len != data.len() {
                        return Err(Error::ReadError);
                    }
                    Ok(Some(data))
                }
                _ => Ok(None), // 其他状态(如已删除)返回None
            },
            None => return Ok(None), // 键不存在
        }
    }

    /// 删除数据库中的键值对
    ///
    /// 在KVDB内部实现中，删除操作并不会立即从存储中移除数据，而是将其标记为删除状态，
    /// 因此删除后数据库占用空间不会立即变化。
    ///
    /// # 参数
    /// - `key`: 要删除的键
    ///
    /// # 返回值
    /// 成功时返回Ok(())，失败时返回Error
    pub fn delete<K: AsRef<str>>(&mut self, key: K) -> Result<(), Error> {
        let key = key.as_ref();
        let c_key = CString::new(key)?; // 转换为C字符串
        Error::convert(unsafe { fdb_kv_del(self.handle(), c_key.as_ptr()) })
    }

    /// 重置KVDB到初始状态
    ///
    /// 将数据库中的所有键值对恢复为首次初始化时的默认值
    ///
    /// # 返回值
    /// 成功时返回Ok(())，失败时返回Error
    pub fn reset(&mut self) -> Result<(), Error> {
        Error::convert(unsafe { fdb_kv_set_default(self.handle()) })
    }

    /// 获取键对应值的读取器
    ///
    /// 用于流式读取大型值，避免一次性加载到内存
    ///
    /// # 参数
    /// - `key`: 要读取的键
    ///
    /// # 返回值
    /// 成功时返回KVReader实例，失败时返回Error
    pub fn get_reader<K: AsRef<str>>(&mut self, key: K) -> Result<KVReader, Error> {
        let key = key.as_ref();
        let c_key = CString::new(key)?;

        // 获取键值对元数据
        let mut kv_obj = unsafe { core::mem::zeroed::<fdb_kv>() };
        if unsafe { fdb_kv_get_obj(self.handle(), c_key.as_ptr(), &mut kv_obj) }
            == core::ptr::null_mut()
        {
            return Err(Error::ReadError);
        };

        Ok(KVReader {
            inner: self,
            kv_obj: kv_obj,
            position: 0, // 初始读取位置
            _marker: Default::default(),
        })
    }

    /// 创建数据库迭代器
    ///
    /// 用于遍历数据库中的所有键值对
    ///
    /// # 返回值
    /// KVDBIterator实例
    pub fn iter(&mut self) -> KVDBIterator {
        let mut iterator: fdb_kv_iterator = unsafe { core::mem::zeroed() };
        unsafe { fdb_kv_iterator_init(self.handle(), &mut iterator) }; // 初始化迭代器
        KVDBIterator {
            inner: self,
            iterator,
            is_done: false,
        }
    }

    /// 内部方法：获取键对应的KV对象
    #[inline]
    fn fdb_kv_get_obj<K: AsRef<str>>(&mut self, key: K) -> Result<Option<fdb_kv>, Error> {
        let key = key.as_ref();
        let c_key = CString::new(key)?;
        let mut kv_obj = unsafe { core::mem::zeroed::<fdb_kv>() };

        // 调用底层C函数获取KV对象
        if unsafe { fdb_kv_get_obj(self.handle(), c_key.as_ptr(), &mut kv_obj) }
            == core::ptr::null_mut()
        {
            return Ok(None);
        };
        Ok(Some(kv_obj))
    }

    /// 内部方法：通过blob写入键值对
    #[inline]
    fn fdb_blob_write<K: AsRef<str>>(&mut self, key: K, blob: &mut fdb_blob) -> Result<(), Error> {
        let key = key.as_ref();
        let c_key = CString::new(key)?;
        Error::convert(unsafe { fdb_kv_set_blob(self.handle(), c_key.as_ptr(), blob) })
    }

    /// 内部方法：从blob读取数据
    #[inline]
    fn fdb_blob_read(&mut self, blob: &mut fdb_blob) -> usize {
        unsafe { fdb_blob_read(self.handle() as *mut _, blob) }
    }

    /// 内部方法：控制数据库写入操作
    #[inline]
    fn fdb_kvdb_control_write<T>(&mut self, cmd: u32, arg: T) {
        fdb_kvdb_control_write(self.handle(), cmd, arg)
    }

    /// 内部方法：控制数据库读取操作
    #[inline]
    fn fdb_kvdb_control_read<T>(&self, cmd: u32, arg: &mut T) {
        fdb_kvdb_control_read(self.handle(), cmd, arg)
    }
}

impl RawHandle for KVDB {
    type Handle = fdb_kvdb_t;

    /// 获取底层C库的数据库指针
    fn handle(&self) -> Self::Handle {
        &self.inner as *const _ as *mut _
    }
}

impl Drop for KVDB {
    /// 释放数据库资源
    fn drop(&mut self) {
        unsafe { fdb_kvdb_deinit(self.handle()) }; // 调用底层C库的销毁函数
    }
}

/// 标记KVDB为可安全跨线程发送
unsafe impl Send for KVDB {}

/// KV值读取器
///
/// 实现了embedded-io的Read和Seek trait，用于流式读取KV值，适合处理大型数据
/// 生命周期`'a`确保读取器不会超过其关联的KVDB实例的生命周期
pub struct KVReader<'a> {
    inner: *mut KVDB,                   // 指向KVDB实例的指针
    kv_obj: fdb_kv,                     // KV对象元数据
    position: usize,                    // 当前读取位置
    _marker: PhantomData<&'a mut KVDB>, // 生命周期标记
}

impl<'a> embedded_io::ErrorType for KVReader<'a> {
    type Error = Error;
}

impl<'a> embedded_io::Read for KVReader<'a> {
    /// 从KV值中读取数据到缓冲区
    ///
    /// # 参数
    /// - `buf`: 接收数据的缓冲区
    ///
    /// # 返回值
    /// 成功时返回读取的字节数，失败时返回Error
    fn read(&mut self, buf: &mut [u8]) -> Result<usize, Self::Error> {
        if self.position >= self.kv_obj.value_len as usize {
            return Ok(0); // EOF
        }
        let db = unsafe { &mut *self.inner };
        let mut blob = fdb_blob_make_by(buf, &self.kv_obj, self.position);
        let read_len = unsafe { fdb_blob_read(db.handle() as *mut _, &mut blob) };
        self.position += read_len;
        Ok(read_len)
    }
}

impl<'a> embedded_io::Seek for KVReader<'a> {
    /// 调整读取位置
    ///
    /// # 参数
    /// - `pos`: 要定位的位置，支持从开始、当前位置或末尾偏移
    ///
    /// # 返回值
    /// 成功时返回新的位置，失败时返回Error
    fn seek(&mut self, pos: embedded_io::SeekFrom) -> Result<u64, Self::Error> {
        let total_len = self.kv_obj.value_len as usize;
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

/// KVDB迭代器
///
/// 用于遍历数据库中的所有键值对，支持迭代访问所有有效的键值条目
pub struct KVDBIterator<'a> {
    inner: &'a mut KVDB,       // 数据库实例的可变引用
    iterator: fdb_kv_iterator, // 底层C库的迭代器结构体
    is_done: bool,             // 迭代是否已完成的标志
}

/// 迭代器产出的键值条目
///
/// 包含键的字符串表示和值的读取器，通过读取器可以流式访问值数据
pub struct KVEntry<'a> {
    pub key: String,          // 键的字符串表示
    pub reader: KVReader<'a>, // 值的流式读取器
}

impl<'a> Iterator for KVDBIterator<'a> {
    type Item = Result<KVEntry<'a>, Error>;

    /// 获取下一个键值条目
    ///
    /// # 返回值
    /// 迭代未完成时返回Some(Ok(KVEntry))或错误，完成时返回None
    fn next(&mut self) -> Option<Self::Item> {
        if self.is_done {
            return None;
        }

        // 调用底层C库函数获取下一个条目
        if !unsafe { fdb_kv_iterate(self.inner.handle(), &mut self.iterator) } {
            self.is_done = true;
            return None;
        }

        // 克隆当前键值对数据
        let curr_kv = self.iterator.curr_kv.clone();
        // 从C字符串构建Rust字符串切片
        let key_slice = unsafe {
            core::slice::from_raw_parts(
                curr_kv.name.as_ptr() as *const u8,
                curr_kv.name_len as usize,
            )
        };

        // 转换为有效的UTF-8字符串
        let key = match core::str::from_utf8(key_slice) {
            Ok(s) => s.to_string(),
            Err(_) => return Some(Err(Error::KvNameError)),
        };

        // 创建值的读取器
        let reader = KVReader {
            inner: self.inner,
            kv_obj: curr_kv,
            position: 0,
            _marker: PhantomData,
        };

        Some(Ok(KVEntry { key, reader }))
    }
}
