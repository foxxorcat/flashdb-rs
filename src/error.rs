use alloc::{ffi::NulError, string::String};
use thiserror::Error;

use crate::{
    fdb_err_t, fdb_err_t_FDB_ERASE_ERR, fdb_err_t_FDB_INIT_FAILED, fdb_err_t_FDB_KV_NAME_ERR,
    fdb_err_t_FDB_KV_NAME_EXIST, fdb_err_t_FDB_NO_ERR, fdb_err_t_FDB_PART_NOT_FOUND,
    fdb_err_t_FDB_READ_ERR, fdb_err_t_FDB_SAVED_FULL, fdb_err_t_FDB_WRITE_ERR,
};

#[derive(Error, Debug)]
pub enum Error {
    #[error("Ok")]
    Ok,
    #[error("Erase operation failed")]
    EraseError,
    #[error("Read operation failed")]
    ReadError,
    #[error("Write operation failed")]
    WriteError,
    #[error("Partition not found")]
    PartNotFound,
    #[error("Invalid KV name")]
    KvNameError,
    #[error("KV name already exists")]
    KvNameExist,
    #[error("Storage full")]
    SavedFull,
    #[error("Initialization failed")]
    InitFailed,
    #[error("Unknown error")]
    UnknownError,
    #[error("Invalid argument")]
    InvalidArgument,
    #[error("Key not found")]
    KeyNotFound,
    #[error("Locking error: {0}")]
    LockingError(String),
}

impl Error {
    pub fn convert(error: fdb_err_t) -> Result<(), Error> {
        if error == fdb_err_t_FDB_NO_ERR {
            Ok(())
        } else {
            Err(error.into())
        }
    }

    pub fn check_and_return<T>(error: fdb_err_t, value: T) -> Result<T, Self> {
        if error == fdb_err_t_FDB_NO_ERR {
            Ok(value)
        } else {
            Err(error.into())
        }
    }
}

impl From<NulError> for Error {
    fn from(_: NulError) -> Self {
        Error::InvalidArgument
    }
}

// 为 C 类型实现 Rust 转换
impl From<fdb_err_t> for Error {
    fn from(err: fdb_err_t) -> Self {
        match err {
            fdb_err_t_FDB_NO_ERR => Error::Ok,
            fdb_err_t_FDB_ERASE_ERR => Error::EraseError,
            fdb_err_t_FDB_READ_ERR => Error::ReadError,
            fdb_err_t_FDB_WRITE_ERR => Error::WriteError,
            fdb_err_t_FDB_PART_NOT_FOUND => Error::PartNotFound,
            fdb_err_t_FDB_KV_NAME_ERR => Error::KvNameError,
            fdb_err_t_FDB_KV_NAME_EXIST => Error::KvNameExist,
            fdb_err_t_FDB_SAVED_FULL => Error::SavedFull,
            fdb_err_t_FDB_INIT_FAILED => Error::InitFailed,
            _ => Error::UnknownError,
        }
    }
}

impl embedded_io::Error for Error {
    fn kind(&self) -> embedded_io::ErrorKind {
        match self {
            Error::ReadError => embedded_io::ErrorKind::Other,
            Error::WriteError => embedded_io::ErrorKind::Other,
            Error::EraseError => embedded_io::ErrorKind::Other,
            Error::InitFailed => embedded_io::ErrorKind::Other,
            Error::PartNotFound => embedded_io::ErrorKind::NotFound,
            Error::KeyNotFound => embedded_io::ErrorKind::NotFound,
            Error::KvNameError => embedded_io::ErrorKind::InvalidInput,
            Error::KvNameExist => embedded_io::ErrorKind::AlreadyExists,
            Error::SavedFull => embedded_io::ErrorKind::OutOfMemory,
            Error::InvalidArgument => embedded_io::ErrorKind::InvalidInput,
            Error::UnknownError => embedded_io::ErrorKind::Other,
            Error::LockingError(_) => embedded_io::ErrorKind::Other,
            Error::Ok => embedded_io::ErrorKind::Other, // 这是一个特殊情况，通常不应作为错误返回
        }
    }
}