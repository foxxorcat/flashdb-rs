use super::fdb_time_t;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

extern "C" fn get_time_wrapper() -> fdb_time_t {
    // 使用系统时间或自定义实现
    current_timestamp()
}

/// FlashDB 时间类型封装
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct FlashDBTime(fdb_time_t);

impl FlashDBTime {
    /// 获取当前时间戳
    pub fn now() -> Self {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs() as fdb_time_t)
            .unwrap_or(0)
            .into()
    }

    /// 从原始值创建
    pub fn from_raw(value: fdb_time_t) -> Self {
        Self(value)
    }

    /// 获取原始值
    pub fn as_raw(&self) -> fdb_time_t {
        self.0
    }
}

impl From<fdb_time_t> for FlashDBTime {
    fn from(value: fdb_time_t) -> Self {
        Self(value)
    }
}

impl From<FlashDBTime> for fdb_time_t {
    fn from(value: FlashDBTime) -> Self {
        value.0
    }
}

impl From<Duration> for FlashDBTime {
    fn from(value: Duration) -> Self {
        Self(value.as_secs() as i32)
    }
}

/// 获取当前时间戳 (C 兼容)
pub fn current_timestamp() -> fdb_time_t {
    FlashDBTime::now().as_raw()
}
