use embedded_storage::nor_flash::NorFlash;

use crate::{fdb_kv_iterate, fdb_kv_iterator, Error, RawHandle};

use super::{KVEntry, KVReader, KVDB};

pub struct KVDBIterator<'a, S: NorFlash> {
    inner: &'a mut KVDB<S>,    // 数据库实例的可变引用
    iterator: fdb_kv_iterator, // 底层C库的迭代器结构体
    is_done: bool,             // 迭代是否已完成的标志
}

impl<'a, S: NorFlash> KVDBIterator<'a, S> {
    pub fn new(inner: &'a mut KVDB<S>) -> Self {
        Self {
            inner,
            iterator: Default::default(),
            is_done: false,
        }
    }
}

impl<'a, S: NorFlash> KVDBIterator<'a, S> {
    pub fn next_reader<'s>(&'s mut self) -> Option<Result<KVReader<'s, S>, Error>> {
        if self.is_done {
            return None;
        }

        // 调用 C 库函数更新迭代器内部状态
        if !unsafe { fdb_kv_iterate(self.inner.handle(), &mut self.iterator) } {
            self.is_done = true;
            return None;
        }

        Some(Ok(KVReader::new(self.inner, self.iterator.curr_kv.into())))
    }
}

impl<'a, S: NorFlash> Iterator for KVDBIterator<'a, S> {
    type Item = KVEntry;

    fn next(&mut self) -> Option<Self::Item> {
        if self.is_done {
            return None;
        }

        // 调用 C 库函数更新迭代器内部状态
        if !unsafe { fdb_kv_iterate(self.inner.handle(), &mut self.iterator) } {
            self.is_done = true;
            return None;
        }
        return Some(self.iterator.curr_kv.into());
    }
}
