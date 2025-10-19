use crate::{
    fdb_kv, fdb_kv_status, fdb_kv_status_FDB_KV_DELETED, fdb_kv_status_FDB_KV_ERR_HDR, fdb_kv_status_FDB_KV_PRE_DELETE, fdb_kv_status_FDB_KV_PRE_WRITE, fdb_kv_status_FDB_KV_UNUSED, fdb_kv_status_FDB_KV_WRITE, fdb_kv_t, RawHandle
};

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

#[derive(Debug, Clone, Default)]
pub struct KVEntry {
    pub(super) inner: fdb_kv,
}

impl KVEntry {
    /// 获取 KV 的当前状态。
    pub fn status(&self) -> KVStatus {
        self.inner.status.into()
    }

    /// 获取 KV 值的字节长度。
    pub fn value_len(&self) -> usize {
        self.inner.value_len as usize
    }

    /// 获取 KV 的名称（键）。
    ///
    /// 返回一个字符串切片 `&str`。如果名称不是有效的 UTF-8 编码，
    /// 此方法会返回 `None`。
    pub fn name(&self) -> Option<&str> {
        // C 库保证了 name_len 是准确的长度
        let name_slice = &self.inner.name[..self.inner.name_len as usize];
        // 将 C 风格的 char 数组（在这里是 u8）转换为 Rust 字符串切片
        core::str::from_utf8(unsafe {
            // fdb_kv.name 的类型是 [i8; 64]，需要安全地转换为 [u8]
            core::slice::from_raw_parts(name_slice.as_ptr() as *const u8, name_slice.len())
        })
        .ok()
    }

    /// 检查内部的 CRC 校验是否通过。
    pub fn is_valid(&self) -> bool {
        self.inner.crc_is_ok
    }
}

impl RawHandle for KVEntry {
    type Handle = fdb_kv_t;
    fn handle(&self) -> Self::Handle {
        &self.inner as *const _ as *mut _
    }
}

impl From<fdb_kv> for KVEntry {
    fn from(value: fdb_kv) -> Self {
        Self { inner: value }
    }
}
