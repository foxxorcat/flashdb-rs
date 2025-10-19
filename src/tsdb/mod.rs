mod types;
pub use types::*;

mod reader;
pub use reader::*;

use crate::{
    fdb_blob, fdb_blob_make_write, fdb_blob_read, fdb_db_t, fdb_tsdb, fdb_tsdb_control_read,
    fdb_tsdb_control_write, fdb_tsdb_deinit, fdb_tsdb_init, fdb_tsdb_t, fdb_tsl_append_with_ts,
    fdb_tsl_clean, fdb_tsl_iter, fdb_tsl_iter_by_time, fdb_tsl_iter_reverse, fdb_tsl_query_count,
    fdb_tsl_set_status, Error, FlashDispatch, RawHandle, FDB_KV_NAME_MAX,
    FDB_TSDB_CTRL_GET_LAST_TIME, FDB_TSDB_CTRL_GET_ROLLOVER, FDB_TSDB_CTRL_GET_SEC_SIZE,
    FDB_TSDB_CTRL_SET_MAX_SIZE, FDB_TSDB_CTRL_SET_NOT_FORMAT, FDB_TSDB_CTRL_SET_ROLLOVER,
    FDB_TSDB_CTRL_SET_SEC_SIZE,
};

use core::{
    ffi::{c_char, c_void},
    marker::PhantomData,
};

use embedded_storage::nor_flash::NorFlash;

pub struct TSDB<S: NorFlash> {
    inner: fdb_tsdb,
    storage: S,
    user_data: FlashDispatch,
    #[cfg(feature = "log")]
    name_buf: [u8; FDB_KV_NAME_MAX as usize + 1],
    initialized: bool,
    // 由于 fdb_kvdb 内部引用了 storage 和 name_buf，结构体无法安全地在线程间移动，
    // 因此标记为 !Send 和 !Sync。
    _marker: PhantomData<*const ()>,
}

#[cfg(feature = "std")]
impl TSDB<crate::storage::StdStorage> {
    /// 在 `std` 环境下，创建一个基于文件的 TSDB 实例。
    ///
    /// 此函数返回一个 `Box<TSDB<StdStorage>>`，以确保数据库实例在内存中的地址是稳定的，
    /// 防止因栈帧移动导致传递给 C 库的内部指针失效。
    ///
    /// # 参数
    /// - `name`: 数据库名称
    /// - `path`: 数据库文件存储的目录
    /// - `sec_size`: 扇区大小
    /// - `max_size`: 数据库最大容量
    /// - `entry_max`: 单个日志条目的最大长度
    pub fn new_file(
        name: &str,
        path: &str,
        sec_size: u32,
        max_size: u32,
        entry_max: usize,
    ) -> Result<Box<Self>, Error> {
        let storage = crate::storage::StdStorage::new(
            path,
            name,
            sec_size,
            max_size,
            crate::storage::FileStrategy::Multi,
        )?;

        let mut db = Box::new(TSDB::new(storage));
        db.set_name(name)?;
        db.init(entry_max)?;
        Ok(db)
    }
}

impl<S: NorFlash> TSDB<S> {
    /// 创建一个未初始化的 KVDB 实例。
    ///
    /// # Arguments
    /// * `storage` - 一个实现了 `NorFlash` trait 的存储实例。
    pub fn new(storage: S) -> Self {
        Self {
            inner: Default::default(),
            storage,
            user_data: FlashDispatch::new::<S>(),
            #[cfg(feature = "log")]
            name_buf: [0; FDB_KV_NAME_MAX as usize + 1],
            initialized: false,
            _marker: PhantomData,
        }
    }

    /// 设置数据库名称，仅用于日志输出。
    ///
    /// **注意**: 此方法必须在 `init()` 之前调用。
    pub fn set_name(&mut self, name: &str) -> Result<(), Error> {
        if name.len() > FDB_KV_NAME_MAX as usize {
            return Err(Error::KvNameError);
        }
        #[cfg(feature = "log")]
        {
            self.name_buf[..name.len()].copy_from_slice(name.as_bytes());
            self.name_buf[name.len()] = b'\0';
        }
        Ok(())
    }

    /// 设置数据库为不可格式化模式。
    ///
    /// 在此模式下，如果数据库初始化时发现头部信息损坏，将返回错误而不是自动格式化。
    /// **注意**: 此方法必须在 `init()` 之前调用。
    pub fn set_not_formatable(&mut self, enable: bool) {
        self.fdb_tsdb_control_write(FDB_TSDB_CTRL_SET_NOT_FORMAT, enable);
    }

    /// 检查数据库是否处于不可格式化模式。
    pub fn not_formatable(&mut self) -> bool {
        let mut enable = false;
        self.fdb_tsdb_control_read(FDB_TSDB_CTRL_SET_NOT_FORMAT, &mut enable);
        return enable;
    }

    /// 启用或禁用翻转写入 (Rollover)。
    ///
    /// 启用后，当数据库写满时，最旧的数据将被新数据覆盖。
    /// 禁用后，数据库写满时 `append` 操作将返回 `SavedFull` 错误。
    /// 默认启用。
    pub fn set_rollover(&mut self, enable: bool) {
        self.fdb_tsdb_control_write(FDB_TSDB_CTRL_SET_ROLLOVER, enable);
    }

    /// 检查翻转写入 (Rollover) 是否已启用。
    pub fn rollover(&self) -> bool {
        let mut flag = false;
        self.fdb_tsdb_control_read(FDB_TSDB_CTRL_GET_ROLLOVER, &mut flag);
        flag
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

    /// 初始化数据库。
    ///
    /// 此方法会加载现有数据库或根据 `storage` 的容量创建一个新的数据库。
    /// 它是实际与底层 C 库交互的入口。
    ///
    /// # 参数
    /// - `default_kvs`: (可选) 提供一组默认键值对。如果数据库是首次创建，
    ///   这些键值对将被写入数据库。
    pub fn init(&mut self, entry_max: usize) -> Result<(), Error> {
        if self.initialized {
            return Ok(());
        }
        // 从 NorFlash trait 获取扇区大小和总容量
        let sec_size = S::ERASE_SIZE as u32;
        let max_size = self.storage.capacity() as u32;

        unsafe {
            let db_ptr = self.handle() as fdb_db_t;
            (*db_ptr).mode = crate::fdb_storage_type_FDB_STORAGE_CUSTOM;

            // 设置 flashdb 的配置
            self.fdb_tsdb_control_write(FDB_TSDB_CTRL_SET_SEC_SIZE, sec_size);
            self.fdb_tsdb_control_write(FDB_TSDB_CTRL_SET_MAX_SIZE, max_size);

            // 只有这里获取才不会导致悬空指针
            self.user_data.instance = &mut self.storage as *mut _ as *mut c_void;

            #[cfg(feature = "log")]
            let name = self.name_buf.as_ptr() as *const c_char;
            #[cfg(not(feature = "log"))]
            let name = b"\0".as_ptr() as *const c_char;

            let result = fdb_tsdb_init(
                db_ptr as *mut fdb_tsdb,
                name,
                core::ptr::null(),
                None,
                entry_max,
                &mut self.user_data as *mut _ as *mut c_void,
            );

            if result == crate::fdb_err_t_FDB_NO_ERR {
                self.initialized = true;
                Ok(())
            } else {
                Err(result.into())
            }
        }
    }
}

impl<S: NorFlash> TSDB<S> {
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

impl<S: NorFlash> TSDB<S> {
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
    pub fn set_status(&mut self, tsl: &mut TSLEntry, status: TSLStatus) -> Result<(), Error> {
        // 调用底层函数设置TSL状态
        Error::convert(unsafe { fdb_tsl_set_status(self.handle(), tsl.handle(), status as _) })
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
    pub fn tsdb_iter<F: FnMut(&mut TSDB<S>, &mut TSLEntry) -> bool + Send>(
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
                    Some(iter_callback::<S, F>),
                    &mut callback_data as *mut _ as *mut _,
                )
            } else {
                fdb_tsl_iter(
                    db,
                    Some(iter_callback::<S, F>),
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
    //
    pub fn tsdb_iter_by_time<F: FnMut(&mut TSDB<S>, &mut TSLEntry) -> bool + Send>(
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
                Some(iter_callback::<S, F>),
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

    /// 获取指定TSL条目的数据
    ///
    /// # 参数
    /// - `tsl_obj`: TSL对象（包含状态和长度信息）
    ///
    /// # 返回
    /// - `Ok(Some(data))`: 状态有效时返回数据
    /// - `Ok(None)`: 状态为UNUSED/DELETED时返回None
    /// - `Err(Error)`: 读取失败（如数据损坏）
    #[cfg(feature = "alloc")]
    pub fn get_value(&mut self, tsl_obj: &TSLEntry) -> Result<Option<alloc::vec::Vec<u8>>, Error> {
        // 转换TSL状态（unsafe操作，确保枚举值匹配）
        let status = unsafe { core::mem::transmute(tsl_obj.status()) };
        match status {
            // 可读取状态（PRE_WRITE/Write/UserStatus1）
            TSLStatus::PRE_WRITE | TSLStatus::Write | TSLStatus::UserStatus1 => {
                // 创建指定长度的缓冲区
                let mut data: alloc::vec::Vec<u8> = alloc::vec::Vec::with_capacity(tsl_obj.value_len());
                unsafe { data.set_len(tsl_obj.value_len()) };
                // 根据TSL创建Blob读取结构
                let mut blob = fdb_blob_make_by_tsl(&mut data, tsl_obj, 0);

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

    /// 打开TSL数据读取器
    ///
    /// # 参数
    /// - `tsl_obj`: 目标TSL对象（通过迭代获取）
    ///
    /// # 返回
    /// - `TSDBReader`: 实现了`Read`和`Seek`的读取器
    pub fn open_read(&mut self, entry: TSLEntry) -> TSDBReader<'_, S> {
        TSDBReader::new(self, entry)
    }
}

impl<S: NorFlash> RawHandle for TSDB<S> {
    type Handle = fdb_tsdb_t;

    fn handle(&self) -> Self::Handle {
        &self.inner as *const _ as *mut _
    }
}

impl<S: NorFlash> Drop for TSDB<S> {
    fn drop(&mut self) {
        if self.initialized {
            unsafe {
                fdb_tsdb_deinit(self.handle());
            };
        }
    }
}
