mod reader;
pub use reader::*;
mod types;
pub use types::*;
mod iter;
pub use iter::*;

use crate::{
    fdb_blob, fdb_blob__bindgen_ty_1, fdb_blob_read, fdb_db_t, fdb_kv, fdb_kv_del, fdb_kv_get_obj,
    fdb_kv_set_blob, fdb_kv_set_default, fdb_kvdb, fdb_kvdb_control_read, fdb_kvdb_control_write,
    fdb_kvdb_deinit, fdb_kvdb_init, Error, FlashDispatch, RawHandle, FDB_KVDB_CTRL_SET_MAX_SIZE,
    FDB_KVDB_CTRL_SET_NOT_FORMAT, FDB_KVDB_CTRL_SET_SEC_SIZE, FDB_KV_NAME_MAX,
};
use core::{
    ffi::{c_char, c_void, CStr},
    marker::PhantomData,
};

use embedded_storage::nor_flash::NorFlash;

pub struct KVDB<S: NorFlash> {
    inner: fdb_kvdb,
    storage: S,
    user_data: FlashDispatch,
    key_buf: [u8; FDB_KV_NAME_MAX as usize + 1],
    #[cfg(feature = "log")]
    name_buf: [u8; FDB_KV_NAME_MAX as usize + 1],
    initialized: bool,
    // 由于fdb_kvdb内部引用了 storage 和 name_buf 所以结构体无法移动，否则会导致悬空指针
    _marker: PhantomData<*const ()>, // for !Send and !Sync
}

#[cfg(feature = "std")]
impl KVDB<crate::storage::StdStorage> {
    /// 在 `std` 环境下，创建一个基于文件的 KVDB 实例。
    ///
    /// 此函数返回一个 `Box<KVDB<StdStorage>>`，以确保数据库实例在内存中的地址是稳定的，
    /// 防止因栈帧移动导致传递给 C 库的内部指针失效。
    ///
    /// # 参数
    /// - `name`: 数据库名称
    /// - `path`: 数据库文件存储的目录
    /// - `sec_size`: 扇区大小
    /// - `max_size`: 数据库最大容量
    /// - `default_kvs`: 可选的默认键值对
    pub fn new_file(
        name: &str,
        path: &str,
        sec_size: u32,
        max_size: u32,
        default_kvs: Option<&'static crate::fdb_default_kv>,
    ) -> Result<Box<Self>, Error> {
        let storage = crate::storage::StdStorage::new(
            path,
            name,
            sec_size,
            max_size,
            crate::storage::FileStrategy::Multi,
        )?;

        let mut db = Box::new(KVDB::new(storage));
        db.set_name(name)?;
        db.init(default_kvs)?;
        Ok(db)
    }
}

impl<S: NorFlash> KVDB<S> {
    /// 创建一个未初始化的 KVDB 实例。
    ///
    /// # Arguments
    /// * `storage` - 一个实现了 `NorFlash` trait 的存储实例。
    pub fn new(storage: S) -> Self {
        Self {
            inner: Default::default(),
            storage,
            user_data: FlashDispatch::new::<S>(),
            key_buf: [0; FDB_KV_NAME_MAX as usize + 1],
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
        self.fdb_kvdb_control_write(FDB_KVDB_CTRL_SET_NOT_FORMAT, enable);
    }

    /// 检查数据库是否处于不可格式化模式。
    pub fn not_formatable(&mut self) -> bool {
        let mut enable = false;
        self.fdb_kvdb_control_read(FDB_KVDB_CTRL_SET_NOT_FORMAT, &mut enable);
        return enable;
    }
    /// 初始化数据库。
    ///
    /// 此方法会加载现有数据库或根据 `storage` 的容量创建一个新的数据库。
    /// 它是实际与底层 C 库交互的入口。
    ///
    /// # 参数
    /// - `default_kvs`: (可选) 提供一组默认键值对。如果数据库是首次创建，
    ///   这些键值对将被写入数据库。
    pub fn init(
        &mut self,
        default_kvs: Option<&'static crate::fdb_default_kv>,
    ) -> Result<(), Error> {
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
            self.fdb_kvdb_control_write(FDB_KVDB_CTRL_SET_SEC_SIZE, sec_size);
            self.fdb_kvdb_control_write(FDB_KVDB_CTRL_SET_MAX_SIZE, max_size);

            // 只有这里获取才不会导致悬空指针
            self.user_data.instance = &mut self.storage as *mut _ as *mut c_void;

            #[cfg(feature = "log")]
            let name = self.name_buf.as_ptr() as *const c_char;
            #[cfg(not(feature = "log"))]
            let name = b"\0".as_ptr() as *const c_char;

            let default_kvs_ptr = match default_kvs {
                Some(kvs) => kvs as *const _ as *mut _,
                None => core::ptr::null_mut(),
            };

            let result = fdb_kvdb_init(
                db_ptr as *mut fdb_kvdb,
                name,
                core::ptr::null(),
                default_kvs_ptr,
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
impl<S: NorFlash> KVDB<S> {
    /// 内部辅助函数：将 &str 转换为 CStr
    fn to_cstr(&mut self, key: &str) -> Result<&CStr, Error> {
        let key_len = key.len();
        if key_len > FDB_KV_NAME_MAX as usize {
            return Err(Error::KvNameError);
        }
        self.key_buf[..key_len].copy_from_slice(key.as_bytes());
        self.key_buf[key_len] = 0;
        // 安全：我们刚刚确保了 key_buf 是一个有效的以 null 结尾的字符串
        Ok(unsafe { CStr::from_bytes_with_nul_unchecked(&self.key_buf[..key_len + 1]) })
    }

    /// 内部方法：获取键对应的KV对象
    #[inline]
    fn fdb_kv_get_obj(&mut self, key: &str) -> Result<Option<KVEntry>, Error> {
        let handle = self.handle();
        let cstr_key = self.to_cstr(key)?;
        let mut kv_obj = unsafe { core::mem::zeroed::<fdb_kv>() };
        // 调用底层C函数获取KV对象
        if unsafe { fdb_kv_get_obj(handle, cstr_key.as_ptr(), &mut kv_obj) }
            == core::ptr::null_mut()
        {
            return Ok(None);
        };
        Ok(Some(kv_obj.into()))
    }

    /// 内部方法：通过blob写入键值对
    #[inline]
    fn fdb_blob_write(&mut self, key: &str, blob: &mut fdb_blob) -> Result<(), Error> {
        let handle = self.handle();
        let cstr_key = self.to_cstr(key)?;
        Error::convert(unsafe { fdb_kv_set_blob(handle, cstr_key.as_ptr(), blob) })
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
    #[allow(dead_code)]
    fn fdb_kvdb_control_read<T>(&self, cmd: u32, arg: &mut T) {
        fdb_kvdb_control_read(self.handle(), cmd, arg)
    }
}

impl<S: NorFlash> KVDB<S> {
    /// 存储一个键值对。
    ///
    /// 如果键已存在，其值将被覆盖。
    ///
    /// # 参数
    /// - `key`: 键
    /// - `value`: 值，一个字节切片。
    pub fn set(&mut self, key: &str, value: &[u8]) -> Result<(), Error> {
        let mut blob = fdb_blob_make_write(value); // 创建写入用的blob结构
        self.fdb_blob_write(key, &mut blob)
    }

    /// 根据键获取其值。
    ///
    /// # 参数
    /// - `key`: 要查询的键。
    ///
    /// # 返回
    /// - `Ok(Some(Vec<u8>))`: 找到键，返回其值。
    /// - `Ok(None)`: 未找到键。
    /// - `Err(Error)`: 读取时发生错误。
    #[cfg(feature = "alloc")]
    pub fn get(&mut self, key: &str) -> Result<Option<alloc::vec::Vec<u8>>, Error> {
        match self.fdb_kv_get_obj(key)? {
            Some(kv) => match kv.status() {
                // 处理预写入或已写入状态的值
                KVStatus::PRE_WRITE | KVStatus::Write => {
                    // 初始化缓冲区

                    let mut data: alloc::vec::Vec<u8> =
                        alloc::vec::Vec::with_capacity(kv.value_len());
                    unsafe { data.set_len(kv.value_len()) }; // 预分配缓冲区大小

                    // 创建读取用的blob结构
                    let mut blob = fdb_blob_make_by(&mut data, &kv.into(), 0);

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

    /// 删除一个键值对。
    ///
    /// 这是一个逻辑删除，数据占用的空间将在未来的垃圾回收 (GC) 过程中被回收。
    pub fn delete(&mut self, key: &str) -> Result<(), Error> {
        let handle = self.handle();
        let cstr_key = self.to_cstr(key)?;
        Error::convert(unsafe { fdb_kv_del(handle, cstr_key.as_ptr()) })
    }

    /// 重置数据库到其默认状态。
    ///
    /// 如果初始化时提供了默认键值对，数据库将恢复到这些值。
    /// 否则，数据库将被清空。
    ///
    /// **警告**: 此操作会删除所有当前数据。
    pub fn reset(&mut self) -> Result<(), Error> {
        Error::convert(unsafe { fdb_kv_set_default(self.handle()) })
    }

    /// 获取一个用于流式读取键值的 `KVReader`。
    ///
    /// 这对于读取大尺寸的值非常有用，可以避免一次性将整个值加载到内存中。
    pub fn get_reader<'a>(&'_ mut self, key: &str) -> Result<KVReader<'_, S>, Error> {
        let handle = self.handle();
        let cstr_key = self.to_cstr(key)?;
        let mut kv_obj = unsafe { core::mem::zeroed::<fdb_kv>() };
        if unsafe { fdb_kv_get_obj(handle, cstr_key.as_ptr(), &mut kv_obj) }
            == core::ptr::null_mut()
        {
            return Err(Error::ReadError);
        };

        Ok(KVReader::new(self, kv_obj.into()))
    }

    pub fn iter(&mut self) -> KVDBIterator<'_, S> {
        KVDBIterator::new(self)
    }
}

impl<S: NorFlash> RawHandle for KVDB<S> {
    type Handle = *mut fdb_kvdb;
    fn handle(&self) -> Self::Handle {
        &self.inner as *const _ as *mut _
    }
}

impl<S: NorFlash> Drop for KVDB<S> {
    fn drop(&mut self) {
        if self.initialized {
            unsafe {
                fdb_kvdb_deinit(self.handle());
            }
        }
    }
}

pub fn fdb_blob_make_by(v: &mut [u8], kv: &KVEntry, offset: usize) -> fdb_blob {
    fdb_blob {
        buf: v.as_mut_ptr() as *mut _,
        size: v.len(),
        saved: fdb_blob__bindgen_ty_1 {
            meta_addr: kv.inner.addr.start,
            addr: kv.inner.addr.value + offset as u32,
            len: kv.inner.value_len as usize - offset,
        },
    }
}

pub fn fdb_blob_make_read(v: &mut [u8]) -> fdb_blob {
    fdb_blob {
        buf: v.as_mut_ptr() as *mut _,
        size: v.len(),
        saved: unsafe { core::mem::zeroed() },
    }
}
pub fn fdb_blob_make_write(v: &[u8]) -> fdb_blob {
    fdb_blob {
        buf: v.as_ptr() as *const _ as *mut _,
        size: v.len(),
        saved: unsafe { core::mem::zeroed() },
    }
}
