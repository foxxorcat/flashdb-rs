use std::path::Path;

use crate::storage::std_impl::FileStrategy;
use crate::storage::Storage;
use crate::{
    fdb_blob, fdb_blob_make_by_tsl, fdb_blob_make_write, fdb_blob_read, fdb_db_t, fdb_tsdb,
    fdb_tsdb_control_read, fdb_tsdb_control_write, fdb_tsdb_deinit, fdb_tsdb_init, fdb_tsdb_t,
    fdb_tsl, fdb_tsl_append_with_ts, fdb_tsl_clean, fdb_tsl_iter, fdb_tsl_iter_by_time,
    fdb_tsl_iter_reverse, fdb_tsl_query_count, fdb_tsl_set_status, fdb_tsl_status_FDB_TSL_DELETED,
    fdb_tsl_status_FDB_TSL_PRE_WRITE, fdb_tsl_status_FDB_TSL_UNUSED,
    fdb_tsl_status_FDB_TSL_USER_STATUS1, fdb_tsl_status_FDB_TSL_USER_STATUS2,
    fdb_tsl_status_FDB_TSL_WRITE, fdb_tsl_status_t, fdb_tsl_t, Error, RawHandle,
    FDB_TSDB_CTRL_GET_LAST_TIME, FDB_TSDB_CTRL_GET_SEC_SIZE, FDB_TSDB_CTRL_SET_FILE_MODE,
    FDB_TSDB_CTRL_SET_MAX_SIZE, FDB_TSDB_CTRL_SET_NOT_FORMAT, FDB_TSDB_CTRL_SET_ROLLOVER,
    FDB_TSDB_CTRL_SET_SEC_SIZE,
};

use alloc::ffi::CString;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

#[repr(u32)]
#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum TSLStatus {
    UNUSED = fdb_tsl_status_FDB_TSL_UNUSED,
    PRE_WRITE = fdb_tsl_status_FDB_TSL_PRE_WRITE,
    Write = fdb_tsl_status_FDB_TSL_WRITE, // 已写入状态，TSL 被追加到 TSDB 后的默认状态
    UserStatus1 = fdb_tsl_status_FDB_TSL_USER_STATUS1, // 介于写入与删除之间，用户可自定义状态含义（如数据已同步至云端）
    Deleted = fdb_tsl_status_FDB_TSL_DELETED,          // 已删除状态，删除 TSL 时设置此状态
    UserStatus2 = fdb_tsl_status_FDB_TSL_USER_STATUS2, // 介于写入与删除之间，用户可自定义状态含义（如数据已同步至云端）
}

impl From<fdb_tsl_status_t> for TSLStatus {
    fn from(value: fdb_tsl_status_t) -> Self {
        unsafe { core::mem::transmute(value) }
    }
}

/// --------------------------
/// TSDB构建器（Builder模式）
/// 负责配置并创建时间序列数据库实例
/// --------------------------
#[derive(Debug, Clone)]
pub struct TSDBBuilder {
    name: String,         // 数据库名称（标识）
    path: Option<String>, // 存储路径（文件系统或设备）
    entry_max_len: usize, // 单个日志条目的最大字节数
    sec_size: u32,        // 存储扇区大小（影响IO效率）
    max_size: u32,        // 数据库最大文件大小（文件模式）
    not_format: bool,     // 初始化时是否跳过格式化
}

impl TSDBBuilder {
    pub fn new(name: &str, max_size: u32, entry_max_len: usize) -> Self {
        Self {
            name: name.to_string(),
            path: None,
            sec_size: 4096, // 默认扇区大小4096字节
            max_size,
            not_format: false,
            entry_max_len,
        }
    }
    /// 链式设置数据库名称
    pub fn with_name<N: ToString>(mut self, name: N) -> Self {
        self.name = name.to_string();
        self
    }

    /// 链式设置单个条目的最大长度
    pub fn with_entry_max_len(mut self, entry_max_len: usize) -> Self {
        self.entry_max_len = entry_max_len;
        self
    }

    /// 链式设置扇区大小（建议为块设备物理扇区的整数倍）
    pub fn with_sec_size(mut self, sec_size: u32) -> Self {
        self.sec_size = sec_size;
        self
    }

    /// 链式设置最大文件大小（超出时触发rollover）
    pub fn with_max_size(mut self, max_size: u32) -> Self {
        self.max_size = max_size;
        self
    }

    /// 链式设置是否跳过初始化格式化（用于恢复已有数据库）
    pub fn with_not_format(mut self, not_format: bool) -> Self {
        self.not_format = not_format;
        self
    }

    /// 使用指定的存储后端打开数据库
    ///
    /// # 参数
    /// - `storage`: 实现Storage trait的存储后端实例
    ///
    /// # 返回值
    /// 成功时返回KVDB实例，失败时返回Error
    pub fn open_with<S: Storage + 'static>(self, storage: S) -> Result<TSDB, Error> {
        let storage_boxed = Box::new(Box::new(storage) as Box<dyn Storage>);
        let storage_boxed_raw = Box::into_raw(storage_boxed);
        let storage = unsafe { Box::from_raw(storage_boxed_raw) };

        let name = CString::new(self.name).unwrap();
        let path = CString::new(self.path.unwrap_or_default()).unwrap();

        let tsdb = TSDB {
            name,
            path,
            inner: Default::default(),
            storage,
        };

        let entry_max_len = self.entry_max_len;

        unsafe {
            // 获取数据库指针并配置存储回调s
            let db_ptr = tsdb.handle() as fdb_db_t;

            (*db_ptr).mode = crate::fdb_storage_type_FDB_STORAGE_CUSTOM;

            // 设置数据库参数
            (*db_ptr).sec_size = self.sec_size;
            (*db_ptr).max_size = self.max_size;
            (*db_ptr).not_formatable = self.not_format;

            // 初始化数据库
            let result = fdb_tsdb_init(
                db_ptr as fdb_tsdb_t,
                tsdb.name.as_ptr(),
                tsdb.path.as_ptr(),
                None,
                entry_max_len,
                storage_boxed_raw as *mut _,
            );

            Error::check_and_return(result, tsdb)
        }
    }
}

#[cfg(feature = "std")]
impl TSDBBuilder {
    /// 创建文件模式的TSDB构建器（默认配置）
    ///
    /// # 参数
    /// - `name`: 数据库名称
    /// - `path`: 存储路径
    /// - `max_size`: 最大文件大小（字节）
    /// - `entry_max`: 单个条目的最大长度
    pub fn file<S: ToString>(name: S, path: S, max_size: u32, entry_max: usize) -> Self {
        Self {
            name: name.to_string(),
            path: Some(path.to_string()),
            sec_size: 4096, // 默认4KB扇区（常见块设备大小）
            max_size: max_size,
            entry_max_len: entry_max,
            not_format: false,
        }
    }

    /// 链式设置存储路径
    pub fn with_path<S: ToString>(mut self, path: S) -> Self {
        self.path = Some(path.to_string());
        self
    }

    /// 打开数据库实例（核心初始化逻辑）
    ///
    /// # 返回
    /// - `Ok(TSDB)`: 成功创建数据库实例
    /// - `Err(Error)`: 初始化失败（如路径无效、参数错误）
    pub fn open(self) -> Result<TSDB, Error> {
        // 获取存储路径s
        let path = self.path.clone().ok_or(Error::InvalidArgument)?;

        // 创建标准存储后端
        let storage = crate::StdStorage::new(path, &self.name, self.sec_size, FileStrategy::Multi)?;
        self.open_with(storage)
    }
}

/// --------------------------
/// 时间序列数据库核心结构体
/// 封装底层C库接口，提供安全的Rust API
/// --------------------------s
pub struct TSDB {
    name: CString,
    path: CString,
    inner: fdb_tsdb,
    storage: Box<Box<dyn Storage>>,
}

// 迭代器闭包数据包装（用于跨语言回调）
struct CallbackData<'a, F> {
    callback: F,      // 用户提供的迭代回调函数
    db: &'a mut TSDB, // 当前数据库引用
}

/// 跨语言回调函数（unsafe边界）
///
/// # 安全说明
/// - 必须确保`arg`指针指向有效的`CallbackData`
/// - 闭包需实现`Send` trait以支持线程安全
unsafe extern "C" fn iter_callback<F: FnMut(&mut TSDB, &mut fdb_tsl) -> bool + Send>(
    tsl: fdb_tsl_t,
    arg: *mut core::ffi::c_void,
) -> bool {
    // 从C指针还原Rust结构体（unsafe操作）
    let callback_data: &mut CallbackData<'_, F> = unsafe { core::mem::transmute(arg) };
    // 调用用户闭包并传递数据库引用和TSL句柄
    // 这里反转一下，使其更符合rust遍历习惯
    !(callback_data.callback)(callback_data.db, unsafe { core::mem::transmute(tsl) })
}

impl TSDB {
    /// 追加带时间戳的日志条目
    ///
    /// # 参数
    /// - `timestamp`: 时间戳（毫秒级UNIX时间）
    /// - `data`: 要存储的字节数据
    ///
    /// # 返回
    /// - `Ok(())`: 追加成功
    /// - `Err(Error)`: 存储失败（如空间不足）
    pub fn append_with_timestamp(&mut self, timestamp: i64, data: &[u8]) -> Result<(), Error> {
        // 创建可写Blob结构（封装数据缓冲区）
        let mut blob = fdb_blob_make_write(data);
        // 调用底层C函数追加带时间戳的TSL
        Error::convert(unsafe { fdb_tsl_append_with_ts(self.handle(), &mut blob, timestamp as _) })
    }

    /// 设置日志条目的状态（逻辑标记）
    ///
    /// # 参数
    /// - `timestamp`: 目标日志的时间戳
    /// - `status`: 要设置的状态（如已同步、已删除）
    ///
    /// # 应用场景
    /// - 标记数据已上传至云端
    /// - 逻辑删除旧数据（非物理删除）
    pub fn set_status(&mut self, tsl: &mut fdb_tsl, status: TSLStatus) -> Result<(), Error> {
        // 调用底层函数设置TSL状态
        Error::convert(unsafe { fdb_tsl_set_status(self.handle(), tsl as *mut _, status as _) })
    }

    /// 查询指定时间范围内特定状态的日志数量
    ///
    /// # 参数
    /// - `from`: 起始时间戳
    /// - `to`: 结束时间戳
    /// - `status`: 要筛选的状态
    ///
    /// # 注意
    /// - 结果通过底层API直接输出，未返回Rust值
    pub fn count(&mut self, from: i64, to: i64, status: TSLStatus) -> usize {
        unsafe { fdb_tsl_query_count(self.handle(), from as _, to as _, status as _) }
    }

    /// 迭代所有日志条目（支持正向/反向）
    ///
    /// # 参数
    /// - `callback`: 迭代回调函数，返回`false`可提前终止
    /// - `reverse`: 是否反向迭代（最新条目优先）
    ///
    /// # 闭包签名
    /// ```rust
    /// F: Fn(&mut TSDB, *mut fdb_tsl) -> bool
    /// ```
    pub fn tsdb_iter<F: FnMut(&mut TSDB, &mut fdb_tsl) -> bool + Send>(
        &mut self,
        callback: F,
        reverse: bool,
    ) {
        let db = self.handle();
        let mut callback_data = CallbackData { db: self, callback };
        unsafe {
            // 根据标志选择正向/反向迭代器
            if reverse {
                fdb_tsl_iter_reverse(
                    db,
                    Some(iter_callback::<F>),
                    &mut callback_data as *mut _ as *mut _,
                )
            } else {
                fdb_tsl_iter(
                    db,
                    Some(iter_callback::<F>),
                    &mut callback_data as *mut _ as *mut _,
                )
            }
        }
    }

    /// 按时间范围迭代日志条目
    ///
    /// # 参数
    /// - `from`: 起始时间戳
    /// - `to`: 结束时间戳 (包含)
    /// - `callback`: 迭代回调函数 (包含)
    /// -
    ///
    /// # 典型用法
    /// ```rust
    /// db.tsdb_iter_by_time(
    ///     1680000000, 1680086400,
    ///     |db, tsl| { /* 处理TSL数据 */ true }
    /// );
    /// ```
    pub fn tsdb_iter_by_time<F: FnMut(&mut TSDB, &mut fdb_tsl) -> bool + Send>(
        &mut self,
        from: i64,
        to: i64,
        callback: F,
    ) {
        let db = self.handle();
        let mut callback_data = CallbackData { db: self, callback };
        unsafe {
            fdb_tsl_iter_by_time(
                db,
                from as _,
                to as _,
                Some(iter_callback::<F>),
                &mut callback_data as *mut _ as *mut _,
            )
        };
    }

    /// 重置数据库（清除所有日志条目）
    ///
    /// # 警告
    /// - 此操作会删除所有数据，不可恢复
    /// - 建议在初始化或测试时使用
    pub fn reset(&mut self) -> Result<(), Error> {
        unsafe { fdb_tsl_clean(self.handle()) };
        Ok(())
    }

    /// 打开TSL数据读取器
    ///
    /// # 参数
    /// - `tsl_obj`: 目标TSL对象（通过迭代获取）
    ///
    /// # 返回
    /// - `TSDBReader`: 实现了`Read`和`Seek`的读取器
    pub fn open_read(&mut self, tsl_obj: fdb_tsl) -> TSDBReader {
        TSDBReader {
            inner: self,
            tsl_obj,
            position: 0,
            _marker: Default::default(),
        }
    }

    /// 获取指定TSL条目的数据
    ///
    /// # 参数
    /// - `tsl_obj`: TSL对象（包含状态和长度信息）
    ///
    /// # 返回
    /// - `Ok(Some(data))`: 状态有效时返回数据
    /// - `Ok(None)`: 状态为UNUSED/DELETED时返回None
    /// - `Err(Error)`: 读取失败（如数据损坏）
    pub fn get_value(&mut self, tsl_obj: &fdb_tsl) -> Result<Option<Vec<u8>>, Error> {
        // 转换TSL状态（unsafe操作，确保枚举值匹配）
        let status = unsafe { core::mem::transmute(tsl_obj.status) };
        match status {
            // 可读取状态（PRE_WRITE/Write/UserStatus1）
            TSLStatus::PRE_WRITE | TSLStatus::Write | TSLStatus::UserStatus1 => {
                // 创建指定长度的缓冲区
                let mut data: Vec<u8> = Vec::with_capacity(tsl_obj.log_len as _);
                unsafe { data.set_len(tsl_obj.log_len as _) };
                // 根据TSL创建Blob读取结构
                let mut blob = fdb_blob_make_by_tsl(&mut data, &tsl_obj, 0);

                // 执行底层读取
                let read_len = self.fdb_blob_read(&mut blob);
                if read_len != data.len() {
                    return Err(Error::ReadError);
                }
                Ok(Some(data))
            }
            // 不可读取状态（UNUSED/Deleted/UserStatus2）
            TSLStatus::UNUSED | TSLStatus::Deleted | TSLStatus::UserStatus2 => Ok(None),
        }
    }

    /// 获取数据库是否启用rollover功能（文件大小超出时循环覆盖）
    pub fn rollover(&self) -> bool {
        let mut flag = false;
        self.fdb_tsdb_control_read(FDB_TSDB_CTRL_SET_ROLLOVER, &mut flag);
        flag
    }

    /// 设置数据库rollover功能（文件模式专用）
    ///
    /// # 参数
    /// - `disable`: `true`禁用rollover，`false`启用
    ///
    /// # 注意
    /// - 禁用rollover时，数据库写满后会报错
    /// - 启用时会循环覆盖旧数据（类似循环缓冲区）
    pub fn set_rollover(&mut self, disable: bool) {
        self.fdb_tsdb_control_write(FDB_TSDB_CTRL_SET_ROLLOVER, !disable);
    }

    /// 获取当前扇区大小（字节）
    pub fn sec_size(&self) -> u32 {
        let mut size: u32 = 0;
        self.fdb_tsdb_control_read(FDB_TSDB_CTRL_GET_SEC_SIZE, &mut size);
        size
    }

    /// 获取上次追加 TSL 时的时间戳
    pub fn last_time(&self) -> i64 {
        let mut size: i64 = 0;
        self.fdb_tsdb_control_read(FDB_TSDB_CTRL_GET_LAST_TIME, &mut size);
        size
    }

    #[inline]
    fn fdb_blob_read(&mut self, blob: &mut fdb_blob) -> usize {
        unsafe { fdb_blob_read(self.handle() as *mut _, blob) }
    }

    #[inline]
    fn fdb_tsdb_control_write<T>(&mut self, cmd: u32, arg: T) {
        fdb_tsdb_control_write(self.handle(), cmd, arg)
    }

    #[inline]
    fn fdb_tsdb_control_read<T>(&self, cmd: u32, arg: &mut T) {
        fdb_tsdb_control_read(self.handle(), cmd, arg)
    }
}

impl RawHandle for TSDB {
    type Handle = fdb_tsdb_t;

    fn handle(&self) -> Self::Handle {
        &self.inner as *const _ as *mut _
    }
}

impl Drop for TSDB {
    fn drop(&mut self) {
        unsafe {
            fdb_tsdb_deinit(self.handle());
        }
    }
}

unsafe impl Send for TSDB {}
// unsafe impl Sync for TSDB {}

pub struct TSDBReader<'a> {
    inner: *mut TSDB, // 使用原始指针
    tsl_obj: fdb_tsl,
    position: usize,
    _marker: core::marker::PhantomData<&'a mut TSDB>, // 生命周期标记
}

impl<'a> embedded_io::ErrorType for TSDBReader<'a> {
    type Error = Error;
}

impl<'a> embedded_io::Read for TSDBReader<'a> {
    fn read(&mut self, buf: &mut [u8]) -> Result<usize, Self::Error> {
        let total_len = self.tsl_obj.log_len as usize;
        if self.position >= total_len {
            return Ok(0); // EOF
        }

        // 安全：指针生命周期由迭代器保证
        let tsdb = unsafe { &mut *self.inner };
        let mut blob = fdb_blob_make_by_tsl(buf, &self.tsl_obj, self.position);
        let actual_read = tsdb.fdb_blob_read(&mut blob);
        self.position += actual_read;
        Ok(actual_read)
    }
}

impl<'a> embedded_io::Seek for TSDBReader<'a> {
    fn seek(&mut self, pos: embedded_io::SeekFrom) -> Result<u64, Self::Error> {
        let total_len = self.tsl_obj.log_len as usize;
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
