use embedded_storage::nor_flash::NorFlash;

use crate::{
    fdb_blob, fdb_blob__bindgen_ty_1, fdb_tsl, fdb_tsl_status_FDB_TSL_DELETED, fdb_tsl_status_FDB_TSL_PRE_WRITE, fdb_tsl_status_FDB_TSL_UNUSED, fdb_tsl_status_FDB_TSL_USER_STATUS1, fdb_tsl_status_FDB_TSL_USER_STATUS2, fdb_tsl_status_FDB_TSL_WRITE, fdb_tsl_status_t, fdb_tsl_t, RawHandle, TSDB
};

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

#[derive(Debug, Clone, Default)]
pub struct TSLEntry {
    pub(super) inner: fdb_tsl,
}

impl TSLEntry {
    pub fn status(&self) -> TSLStatus {
        self.inner.status.into()
    }

    pub fn value_len(&self) -> usize {
        self.inner.log_len as usize
    }

    pub fn time(&self) -> i64 {
        self.inner.time as i64
    }
}

impl RawHandle for TSLEntry {
    type Handle = fdb_tsl_t;
    fn handle(&self) -> Self::Handle {
        &self.inner as *const _ as *mut _
    }
}

impl From<fdb_tsl> for TSLEntry {
    fn from(value: fdb_tsl) -> Self {
        Self { inner: value }
    }
}

// 迭代器闭包数据包装（用于跨语言回调）
pub(super) struct CallbackData<'a, S: NorFlash, F> {
    pub(super) callback: F,         // 用户提供的迭代回调函数
    pub(super) db: &'a mut TSDB<S>, // 当前数据库引用
}

/// 跨语言回调函数（unsafe边界）
///
/// # 安全说明
/// - 必须确保`arg`指针指向有效的`CallbackData`
/// - 闭包需实现`Send` trait以支持线程安全
pub(super) unsafe extern "C" fn iter_callback<
    S: NorFlash,
    F: FnMut(&mut TSDB<S>, &mut TSLEntry) -> bool + Send,
>(
    tsl: fdb_tsl_t,
    arg: *mut core::ffi::c_void,
) -> bool {
    // 从C指针还原Rust结构体（unsafe操作）
    let callback_data: &mut CallbackData<'_, S, F> = unsafe { core::mem::transmute(arg) };
    // 调用用户闭包并传递数据库引用和TSL句柄
    // 这里反转一下，使其更符合rust遍历习惯
    // 这里可能会导致问题
    !(callback_data.callback)(callback_data.db, unsafe { core::mem::transmute(tsl) })
}


pub fn fdb_blob_make_by_tsl(v: &mut [u8], tsl:& TSLEntry, offset: usize) -> fdb_blob {
    fdb_blob {
        buf: v.as_mut_ptr() as *mut _,
        size: v.len(),
        saved: fdb_blob__bindgen_ty_1 {
            meta_addr: tsl.inner.addr.index,
            addr: tsl.inner.addr.log + offset as u32,
            len: tsl.inner.log_len as usize - offset,
        },
    }
}
