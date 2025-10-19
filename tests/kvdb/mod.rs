#![cfg(feature = "std")]
#![cfg(test)]

use core::ffi::CStr;
use embedded_io::{Read, Seek};
use flashdb_rs::{define_default_kvs, KVDB};
use tempfile::TempDir;

// 使用宏定义一组默认键值对，用于测试
define_default_kvs! {
    MY_DEFAULT_KVS,
    "version" => b"1.0.0",
    "boot_count" => b"0",
}

#[test]
fn test_kvdb_with_default_kvs() -> anyhow::Result<()> {
    let temp_dir = TempDir::new()?;
    let path = temp_dir.path().to_str().unwrap();

    // 1. 使用默认 KVs 初始化一个新的数据库
    let mut db = KVDB::new_file("default_db", path, 4096, 128 * 1024, Some(&MY_DEFAULT_KVS.0))?;

    // 2. 验证默认值是否已成功写入
    let ver = db.get(CStr::from_bytes_with_nul(b"version\0")?)?.unwrap();
    assert_eq!(ver, b"1.0.0");

    let count = db.get(CStr::from_bytes_with_nul(b"boot_count\0")?)?.unwrap();
    assert_eq!(count, b"0");

    // 3. 修改一个默认值并验证
    db.set(CStr::from_bytes_with_nul(b"boot_count\0")?, b"1")?;
    let new_count = db.get(CStr::from_bytes_with_nul(b"boot_count\0")?)?.unwrap();
    assert_eq!(new_count, b"1");

    // 4. 测试 reset 功能是否能恢复默认值
    db.reset()?;
    let reset_count = db.get(CStr::from_bytes_with_nul(b"boot_count\0")?)?.unwrap();
    assert_eq!(reset_count, b"0", "reset 应该能恢复默认值");

    Ok(())
}


#[test]
fn test_kvdb_basic_operations() -> anyhow::Result<()> {
    let temp_dir = TempDir::new()?;
    let path = temp_dir.path().to_str().unwrap();
    let db_name = "test_db";

    let mut db = KVDB::new_file(db_name, path, 4096, 128 * 4096, None)?;

    let key1 = CStr::from_bytes_with_nul(b"key1\0")?;
    let value1 = b"hello";
    db.set(key1, value1)?;

    let key2 = CStr::from_bytes_with_nul(b"key2\0")?;
    let value2 = b"world";
    db.set(key2, value2)?;

    // 测试 Get
    assert_eq!(db.get(key1)?.unwrap(), value1);
    assert_eq!(db.get(key2)?.unwrap(), value2);

    // 测试 Delete
    db.delete(key1)?;
    assert!(db.get(key1)?.is_none());
    assert!(db.get(key2)?.is_some()); // 确保另一个键不受影响

    Ok(())
}

#[test]
fn test_kvdb_iterator() -> anyhow::Result<()> {
    let temp_dir = TempDir::new()?;
    let path = temp_dir.path().to_str().unwrap();
    let mut db = KVDB::new_file("iter_db", path, 4096, 128 * 1024, None)?;

    db.set(CStr::from_bytes_with_nul(b"a\0")?, b"1")?;
    db.set(CStr::from_bytes_with_nul(b"b\0")?, b"2")?;
    db.set(CStr::from_bytes_with_nul(b"c\0")?, b"3")?;

    let mut found_keys = std::collections::HashSet::new();
    let mut total_len = 0;

    for entry in db.iter() {
        let name = entry.name().unwrap().to_string();
        found_keys.insert(name);
        total_len += entry.value_len();
    }

    assert!(found_keys.contains("a"));
    assert!(found_keys.contains("b"));
    assert!(found_keys.contains("c"));
    assert_eq!(found_keys.len(), 3);
    assert_eq!(total_len, 3, "所有值的长度总和应为3");

    Ok(())
}


#[test]
fn test_kvdb_reader_seek() -> anyhow::Result<()> {
    let temp_dir = TempDir::new()?;
    let path = temp_dir.path().to_str().unwrap();
    let mut db = KVDB::new_file("seek_test", path, 4096, 128 * 4096, None)?;
    let key = CStr::from_bytes_with_nul(b"large_data\0")?;
    let value = (0..1024).map(|i| (i % 256) as u8).collect::<Vec<_>>();
    db.set(key, &value)?;

    let mut reader = db.get_reader(key)?;
    let mut buf = vec![0; 100];

    // 从偏移 500 的地方开始读
    reader.seek(embedded_io::SeekFrom::Start(500))?;
    reader.read_exact(&mut buf)?;
    assert_eq!(&buf[..], &value[500..600]);

    // 从当前位置 (600) 向后 seek 100
    reader.seek(embedded_io::SeekFrom::Current(100))?; // Seek to 700
    reader.read_exact(&mut buf)?;
    assert_eq!(&buf[..], &value[700..800]);

    // 从末尾倒数 50 个字节
    reader.seek(embedded_io::SeekFrom::End(-50))?;
    let mut end_buf = vec![0; 50];
    reader.read_exact(&mut end_buf)?;
    assert_eq!(&end_buf, &value[1024 - 50..]);

    Ok(())
}

// GC 测试保持不变，因为它已经覆盖了核心的垃圾回收逻辑
#[test]
fn test_kvdb_garbage_collection() -> anyhow::Result<()> {
    let temp_dir = TempDir::new()?;
    let path = temp_dir.path().to_str().unwrap();

    let mut db = KVDB::new_file("gc_test_db", path, 4096, 3 * 4096, None)?;

    let value = vec![0u8; 1000];

    for i in 0..3 {
        let key_str = format!("key_sector1_{}\0", i);
        let key = CStr::from_bytes_with_nul(key_str.as_bytes())?;
        db.set(key, &value)?;
    }
    db.delete(CStr::from_bytes_with_nul(b"key_sector1_0\0")?)?;
    db.set(
        CStr::from_bytes_with_nul(b"key_sector1_1\0")?,
        b"new_small_value",
    )?;

    for i in 0..3 {
        let key_str = format!("key_sector2_{}\0", i);
        let key = CStr::from_bytes_with_nul(key_str.as_bytes())?;
        db.set(key, &value)?;
    }

    db.set(CStr::from_bytes_with_nul(b"trigger_gc\0")?, &value)?;

    assert!(
        db.get(CStr::from_bytes_with_nul(b"key_sector1_0\0")?)?
            .is_none()
    );
    assert_eq!(
        db.get(CStr::from_bytes_with_nul(b"key_sector1_1\0")?)?
            .unwrap(),
        b"new_small_value"
    );
    assert_eq!(
        db.get(CStr::from_bytes_with_nul(b"key_sector1_2\0")?)?
            .unwrap(),
        value
    );
    assert_eq!(
        db.get(CStr::from_bytes_with_nul(b"trigger_gc\0")?)?
            .unwrap(),
        value
    );

    Ok(())
}