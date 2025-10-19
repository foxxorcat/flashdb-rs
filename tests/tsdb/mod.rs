#![cfg(feature = "std")]
#![cfg(test)]

use anyhow::Result;
use embedded_io::{Read, Seek};
use flashdb_rs::tsdb::{TSDB, TSLEntry, TSLStatus};
use tempfile::TempDir;

#[test]
fn test_tsdb_basic_operations() -> Result<()> {
    let temp_dir = TempDir::new()?;
    let path = temp_dir.path().to_str().unwrap();
    let mut tsdb = TSDB::new_file("basic_test", path, 4096, 128 * 1024, 1024)?;

    let test_data = b"Test data for TSDB";
    tsdb.append_with_timestamp(1686451200, test_data)?;
    tsdb.append_with_timestamp(1686451201, test_data)?;

    assert_eq!(tsdb.count(0, i64::MAX, TSLStatus::Write), 2);

    let mut found = false;
    tsdb.tsdb_iter(
        |db, tsl| {
            let data = db.get_value(tsl).unwrap().unwrap();
            assert_eq!(data, test_data);
            found = true;
            false // 找到一个就终止
        },
        false,
    );
    assert!(found);

    Ok(())
}

#[test]
fn test_tsdb_properties_and_iter_order() -> Result<()> {
    let temp_dir = TempDir::new()?;
    let path = temp_dir.path().to_str().unwrap();
    let sec_size = 4096;
    let mut tsdb = TSDB::new_file("props_test", path, sec_size, 128 * 1024, 256)?;

    // 1. 测试属性
    assert_eq!(tsdb.sec_size(), sec_size);
    assert_eq!(tsdb.last_time(), 0, "新数据库的 last_time 应该为 0");
    assert!(tsdb.rollover(), "Rollover 默认应为 true");

    // 2. 写入数据并验证 last_time
    tsdb.append_with_timestamp(100, b"a")?;
    assert_eq!(tsdb.last_time(), 100);
    tsdb.append_with_timestamp(200, b"b")?;
    assert_eq!(tsdb.last_time(), 200);
    tsdb.append_with_timestamp(300, b"c")?;
    assert_eq!(tsdb.last_time(), 300);

    // 3. 验证正向迭代顺序
    let mut forward_timestamps = Vec::new();
    tsdb.tsdb_iter(
        |_, tsl| {
            forward_timestamps.push(tsl.time());
            true
        },
        false,
    );
    assert_eq!(forward_timestamps, vec![100, 200, 300]);

    // 4. 验证反向迭代顺序
    let mut reverse_timestamps = Vec::new();
    tsdb.tsdb_iter(
        |_, tsl| {
            reverse_timestamps.push(tsl.time());
            true
        },
        true, // 启用反向迭代
    );
    assert_eq!(reverse_timestamps, vec![300, 200, 100]);

    Ok(())
}


#[test]
fn test_tsl_status_management() -> Result<()> {
    let temp_dir = TempDir::new()?;
    let path = temp_dir.path().to_str().unwrap();
    let mut tsdb = TSDB::new_file("status_test", path, 4096, 16 * 1024, 1024)?;

    let timestamp = 1686451200;
    tsdb.append_with_timestamp(timestamp, b"Status management test")?;

    // 1. 通过回调正确地获取要操作的 TSL Entry
    let mut target_tsl: Option<TSLEntry> = None;
    tsdb.tsdb_iter(
        |_, tsl| {
            // 将 tsl 克隆出来，保存到外部变量中
            target_tsl = Some(tsl.clone());
            // 返回 false 来立即停止迭代
            false
        },
        false,
    );

    // 确保我们确实通过回调找到了 TSL
    assert!(target_tsl.is_some(), "未能通过迭代器找到目标 TSL");
    let mut tsl_to_modify = target_tsl.unwrap();
    assert_eq!(tsl_to_modify.status(), TSLStatus::Write);

    // 2. 设置为 UserStatus1 并通过回调进行验证
    tsdb.set_status(&mut tsl_to_modify, TSLStatus::UserStatus1)?;

    let mut status_checked = false;
    tsdb.tsdb_iter_by_time(timestamp, timestamp, |_, tsl| {
        assert_eq!(tsl.status(), TSLStatus::UserStatus1);
        status_checked = true;
        true // 继续迭代（虽然此范围内只有一个）
    });
    assert!(status_checked, "状态未能成功验证为 UserStatus1");

    // 3. 再次获取最新的 TSL Entry 以便进行下一步操作
    let mut tsl_to_delete: Option<TSLEntry> = None;
    tsdb.tsdb_iter_by_time(timestamp, timestamp, |_, tsl| {
        tsl_to_delete = Some(tsl.clone());
        true
    });

    // 4. 设置为 Deleted 并通过回调进行验证
    tsdb.set_status(&mut tsl_to_delete.unwrap(), TSLStatus::Deleted)?;

    let mut deleted_checked = false;
    tsdb.tsdb_iter_by_time(timestamp, timestamp, |db, tsl| {
        assert_eq!(tsl.status(), TSLStatus::Deleted);
        // 处于 Deleted 状态的日志，get_value 应该返回 None
        let value = db.get_value(tsl).unwrap();
        assert!(value.is_none(), "处于 Deleted 状态的日志应该无法读取到值");
        deleted_checked = true;
        true
    });
    assert!(deleted_checked, "状态未能成功验证为 Deleted");

    Ok(())
}

#[test]
fn test_tsdb_edge_cases() -> Result<()> {
    let temp_dir = TempDir::new()?;
    let path = temp_dir.path().to_str().unwrap();
    let max_len = 256;
    let mut tsdb = TSDB::new_file("edge_test", path, 4096, 16 * 1024, max_len)?;

    // 1. 时间戳非单调递增
    tsdb.append_with_timestamp(100, b"data1")?;
    let result = tsdb.append_with_timestamp(99, b"data_invalid");
    assert!(result.is_err(), "非单调递增的时间戳应该失败");

    // 2. 数据大小边界
    tsdb.append_with_timestamp(101, b"")?; // 空数据
    tsdb.append_with_timestamp(102, &vec![0u8; max_len])?; // 刚好最大长度
    let oversized_result = tsdb.append_with_timestamp(103, &vec![0u8; max_len + 1]);
    assert!(oversized_result.is_err(), "超过最大长度的数据应该失败");

    // 3. 验证数据
    let mut count = 0;
    tsdb.tsdb_iter_by_time(101, 102, |db, tsl| {
        let value = db.get_value(tsl).unwrap().unwrap();
        if tsl.time() == 101 {
            assert_eq!(value.len(), 0);
        } else if tsl.time() == 102 {
            assert_eq!(value.len(), max_len);
        }
        count += 1;
        true
    });
    assert_eq!(count, 2, "应找到空数据和最大长度数据");

    Ok(())
}

// rollover 和 reader 的测试保持不变，它们已经覆盖了相关功能
#[test]
fn test_tsdb_rollover_and_capacity() -> Result<()> {
    let temp_dir = TempDir::new()?;
    let path = temp_dir.path().to_str().unwrap();

    let mut tsdb = TSDB::new_file("rollover_test", path, 4096, 16 * 1024, 256)?;
    
    // 禁用 rollover 并测试写满
    tsdb.set_rollover(false); 
    assert!(!tsdb.rollover());
    
    let mut write_count = 0;
    for i in 1.. {
        if tsdb.append_with_timestamp(i, &[0u8; 200]).is_err() {
            break;
        }
        write_count += 1;
    }
    assert!(write_count > 0 && write_count < 100);

    // 重新打开并启用 rollover
    drop(tsdb);
    let mut tsdb = TSDB::new_file("rollover_test", path, 4096, 16 * 1024, 256)?;
    tsdb.set_rollover(true);
    tsdb.reset()?;
    
    // 大量写入以触发翻转
    for i in 1..=100 {
        tsdb.append_with_timestamp(i, &[0u8; 200])?;
    }

    let mut retrieved_timestamps = Vec::new();
    tsdb.tsdb_iter(|_, tsl| {
        retrieved_timestamps.push(tsl.time());
        true
    }, false);

    assert!(!retrieved_timestamps.contains(&1i64), "最旧的数据应该被翻转覆盖");
    assert!(retrieved_timestamps.contains(&100i64), "最新的数据应该存在");
    
    Ok(())
}

#[test]
fn test_tsdb_reader_operations() -> Result<()> {
    let temp_dir = TempDir::new()?;
    let path = temp_dir.path().to_str().unwrap();
    let mut tsdb = TSDB::new_file("reader_test", path, 4096, 32 * 1024, 1024)?;

    let test_data = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ";
    tsdb.append_with_timestamp(1686451200, test_data)?;

    let mut tsl_entry = None;
    tsdb.tsdb_iter(|_, tsl| {
        tsl_entry = Some(tsl.clone());
        false
    }, false);
    
    let mut reader = tsdb.open_read(tsl_entry.unwrap());
    let mut buffer = [0; 10];
    reader.read_exact(&mut buffer)?;
    assert_eq!(&buffer[..], &test_data[0..10]);

    reader.seek(embedded_io::SeekFrom::Current(10))?; // Seek to 20
    let read_len = reader.read(&mut buffer)?;
    assert_eq!(read_len, 6); // Only 6 bytes left
    assert_eq!(&buffer[..read_len], &test_data[20..26]);

    Ok(())
}