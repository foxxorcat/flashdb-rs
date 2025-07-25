use embedded_io::{Read, Seek};
use flashdb_rs::kvdb::KVDBBuilder;
use tempfile::TempDir; // 需要添加 tempfile 依赖

#[test]
fn test_kvdb_basic_operations() -> anyhow::Result<()> {
    // 创建临时目录
    let temp_dir = TempDir::new()?;
    let path = temp_dir.path().to_str().unwrap();
    let db_name = "test_db";

    // 创建数据库实例
    let mut db = KVDBBuilder::file(db_name, path, 128 * 4096)
        .with_sec_size(4096)
        .open()?;

    // 测试 SET 操作
    let key = "test_key";
    let value = b"hello, world!";
    db.set(key, &mut value.to_vec())?;

    // 测试 GET 操作
    let retrieved_value = db.get(key)?.unwrap();
    assert_eq!(retrieved_value, value);

    // 测试 ITERATOR 遍历
    let mut iter = db.iter();
    let mut found = false;

    while let Some(entry) = iter.next() {
        let mut entry = entry?;
        println!("{}", entry.key);
        if entry.key == key {
            let mut buf = vec![0; value.len()];
            entry.reader.read(&mut buf)?;
            assert_eq!(buf, value);
            found = true;
        }
    }
    assert!(found, "Key not found in iterator");

    // 测试 DELETE 操作
    db.delete(key)?;
    assert!(
        db.get(key).is_ok_and(|x| x.is_none()),
        "Deleted key should not exist"
    );

    // 测试 RESET 操作（重置为默认值，需提前设置默认值）
    // 注意：需确保初始化时存在默认值，或在测试中先设置默认值
    db.set("default_key", &mut b"default_value".to_vec())?;
    db.reset()?;

    assert!(
        db.get(key).is_ok_and(|x| x.is_none()),
        "Reset should clear all keys"
    );
    Ok(())
}

#[test]
fn test_kvdb_handling() -> anyhow::Result<()> {
    let temp_dir = TempDir::new()?;
    let path = temp_dir.path().to_str().unwrap();
    let db_name = "error_test_db";

    let builder = KVDBBuilder::file(db_name, path, 128 * 4096).with_sec_size(4096);

    drop(builder.open()?);
    // assert!(builder.clone().open().is_ok(), "Recreate should fail");

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
    let value = vec![b'A'; 1024]; // 1KB 数据
    db.set(key, &mut value.clone())?;

    let mut reader = db.get_reader(key)?;
    let mut buf = vec![0; 512]; // 读取前半部分

    // 测试 SeekFrom::Start
    reader.seek(embedded_io::SeekFrom::Start(0))?;
    let read = reader.read(&mut buf)?;
    assert_eq!(read, 512);
    assert_eq!(&buf[..512], &value[..512]);

    // 测试 SeekFrom::Current
    reader.seek(embedded_io::SeekFrom::Current(256))?; // 移动到 512+256=768 位置
    let read = reader.read(&mut buf)?;
    assert_eq!(read, 256); // 剩余 256 字节
    assert_eq!(&buf[..256], &value[768..1024]);

    // 测试 SeekFrom::End（从末尾倒推）
    reader.seek(embedded_io::SeekFrom::End(-100))?; // 移动到 924 位置
    let mut buf_end = vec![0; 100];
    let read = reader.read(&mut buf_end)?;
    assert_eq!(read, 100);
    assert_eq!(&buf_end, &value[924..1024]);

    Ok(())
}
