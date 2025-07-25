#![cfg(test)]
use embedded_io::{Read, Seek};
use flashdb_rs::kvdb::KVDBBuilder;
use tempfile::TempDir;

#[test]
fn test_kvdb_basic_operations() -> anyhow::Result<()> {
    let temp_dir = TempDir::new()?;
    let path = temp_dir.path().to_str().unwrap();
    let db_name = "test_db";

    let mut db = KVDBBuilder::file(db_name, path, 128 * 4096)
        .with_sec_size(4096)
        .open()?;

    // 1. 测试 SET 和 GET
    let key = "test_key";
    let value = b"hello, world!";
    db.set(key, value)?;

    let retrieved_value = db.get(key)?.unwrap();
    assert_eq!(retrieved_value, *value);

    // 2. 测试覆盖 (Overwrite)
    // 在 FlashDB 中，覆盖一个键实际上是：(1) 将旧值标记为删除 (2) 在新位置写入新值。
    // 这会产生“脏”数据，为后续的 GC 提供回收目标。
    let new_value = b"new world!";
    db.set(key, new_value)?;
    let retrieved_new_value = db.get(key)?.unwrap();
    assert_eq!(retrieved_new_value, *new_value);

    // 3. 测试 ITERATOR 遍历
    let mut iter = db.iter();
    let mut found = false;
    while let Some(entry_result) = iter.next() {
        let mut entry = entry_result?;
        if entry.key == key {
            let mut buf = vec![0; new_value.len()];
            entry.reader.read_exact(&mut buf)?;
            assert_eq!(buf, *new_value);
            found = true;
        }
    }
    assert!(found, "Key not found in iterator");

    // 4. 测试 DELETE
    db.delete(key)?;
    assert!(db.get(key)?.is_none(), "Deleted key should not exist");

    // 5. 测试 RESET
    db.set("default_key", b"default_value")?;
    db.reset()?;
    assert!(db.get("default_key")?.is_none(), "Reset should clear all keys");

    Ok(())
}

#[test]
fn test_kvdb_garbage_collection() -> anyhow::Result<()> {
    let temp_dir = TempDir::new()?;
    let path = temp_dir.path().to_str().unwrap();

    // FlashDB的GC需要一个备用空扇区来移动有效数据。
    // - Sector 1: 将被填满并变“脏”。
    // - Sector 2: 将被填满。
    // - Sector 3: 将作为GC操作的目标空扇区。
    let mut db = KVDBBuilder::file("gc_test_db", path, 3 * 4096)
        .with_sec_size(4096)
        .open()?;

    let value = vec![0u8; 1000]; // 每个值约 1KB

    // 步骤 1: 填满第一个扇区。
    // 一个4KB的扇区大约可以存放3个1KB的值（考虑到头部开销）。
    for i in 0..3 {
        let key = format!("key_sector1_{}", i);
        db.set(&key, &value)?;
    }

    // 步骤 2: 使第一个扇区变“脏”，为GC创造回收条件。
    db.delete("key_sector1_0")?;
    db.set("key_sector1_1", b"new_small_value")?; // 覆盖

    // 步骤 3: 填满第二个扇区。
    for i in 0..3 {
        let key = format!("key_sector2_{}", i);
        db.set(&key, &value)?;
    }

    // 步骤 4: 此时，Sector 1是脏的，Sector 2是满的，Sector 3是空的。
    // 再写入一个KV，由于没有可用空间，将触发GC。
    // GC会选择最脏的Sector 1进行回收：
    // - 将Sector 1中的有效数据("key_sector1_1", "key_sector1_2")移动到空的Sector 3。
    // - 擦除Sector 1，使其变为空闲。
    db.set("trigger_gc", &value)?;

    // 步骤 5: 验证GC后的数据状态。
    assert!(db.get("key_sector1_0")?.is_none(), "已删除的键不应存在");
    assert_eq!(db.get("key_sector1_1")?.unwrap(), b"new_small_value", "被覆盖的键应为新值");
    assert_eq!(db.get("key_sector1_2")?.unwrap(), value, "未修改的数据应继续存在");
    assert_eq!(db.get("trigger_gc")?.unwrap(), value, "触发GC的写入也应成功");

    // 步骤 6: 验证空间确实已回收，可以继续写入新数据。
    db.set("after_gc", b"final_value")?;
    assert_eq!(db.get("after_gc")?.unwrap(), b"final_value");
    
    Ok(())
}

#[test]
fn test_kvdb_reader_seek() -> anyhow::Result<()> {
    let temp_dir = TempDir::new()?;
    let path = temp_dir.path().to_str().unwrap();
    let mut db = KVDBBuilder::file("seek_test", path, 128 * 4096)
        .with_sec_size(4096)
        .open()?;
    let key = "large_data";
    let value = vec![b'X'; 1024]; // 1KB 数据
    db.set(key, &value)?;

    let mut reader = db.get_reader(key)?;
    let mut buf = vec![0; 512];

    // 从头读取
    reader.seek(embedded_io::SeekFrom::Start(0))?;
    reader.read_exact(&mut buf)?;
    assert_eq!(&buf[..], &value[..512]);

    // 从当前位置偏移
    reader.seek(embedded_io::SeekFrom::Current(256))?; // Seek to 512 + 256 = 768
    let read_len = reader.read(&mut buf)?;
    assert_eq!(read_len, 256); // Only 256 bytes left to read
    assert_eq!(&buf[..256], &value[768..]);

    // 从末尾倒数
    reader.seek(embedded_io::SeekFrom::End(-100))?;
    let mut end_buf = vec![0; 100];
    reader.read_exact(&mut end_buf)?;
    assert_eq!(&end_buf, &value[924..]);

    Ok(())
}