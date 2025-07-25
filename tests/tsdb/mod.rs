#![cfg(test)]

use std::i64;
use embedded_io::{Read, Seek};
use anyhow::Result;
use flashdb_rs::fdb_tsl;
use flashdb_rs::tsdb::{TSDBBuilder, TSLStatus};
use tempfile::TempDir;

// 基础功能测试：数据追加与读取
#[test]
fn test_tsdb_basic_operations() -> Result<()> {
    let temp_dir = TempDir::new()?;
    let path = temp_dir.path().to_str().unwrap();
    let mut tsdb = TSDBBuilder::file("basic_test", path, 128 * 1024, 1024).open()?;

    let test_data = b"Test data for TSDB";
    let timestamp1 = 1686451200;
    tsdb.append_with_timestamp(timestamp1, test_data)?;

    let timestamp2 = 1686451201;
    tsdb.append_with_timestamp(timestamp2, test_data)?;

    let count_num = tsdb.count(0, i64::MAX, TSLStatus::Write);
    assert_eq!(count_num, 2, "数据追加后应能通过迭代查询到");

    // 迭代验证
    let mut target_tsl = Default::default();
    let mut found = false;
    tsdb.tsdb_iter(
        |db, tsl| {
            let data = db.get_value(tsl).unwrap().unwrap();
            assert_eq!(data, test_data);
            target_tsl = tsl.clone();
            found = true;
            false // 找到后终止
        },
        false,
    );
    assert!(found);

    // 按时间查询
    let mut time_count = 0;
    tsdb.tsdb_iter_by_time(timestamp2, timestamp2, |_, _| {
        time_count += 1;
        true
    });
    assert_eq!(time_count, 1, "时间范围查询应返回匹配数据");

    // 状态设置与验证
    tsdb.set_status(&mut target_tsl, TSLStatus::UserStatus1)?;
    let mut status_checked = false;
    tsdb.tsdb_iter(
        |_, tsl| {
            if tsl.time == target_tsl.time {
                let status: TSLStatus = tsl.status.into();
                assert_eq!(status, TSLStatus::UserStatus1);
                status_checked = true;
            }
            true
        },
        true, // 反向迭代
    );
    assert!(status_checked, "状态设置应生效");

    Ok(())
}

// 边界条件测试：翻转写入和容量
#[test]
fn test_tsdb_rollover_and_capacity() -> Result<()> {
    let temp_dir = TempDir::new()?;
    let path = temp_dir.path().to_str().unwrap();

    // 配置小容量数据库（16KB）并默认启用 rollover
    let mut tsdb = TSDBBuilder::file("rollover_test", path, 16 * 1024, 256).open()?;
    assert_eq!(tsdb.rollover(), true, "Rollover should be enabled by default");

    let entry_data = vec![0u8; 200];
    // 1. 禁用 rollover 并测试写满
    tsdb.set_rollover(false); 
    assert_eq!(tsdb.rollover(), false, "Rollover should be disabled");
    
    let mut write_count = 0;
    for i in 1.. {
        if tsdb.append_with_timestamp(i, &entry_data).is_ok() {
            write_count += 1;
        } else {
            // 写入失败，说明数据库已满
            break;
        }
    }
    // 确保至少写入了一些数据，但最终会失败
    assert!(write_count > 0 && write_count < 100, "DB should fill up when rollover is disabled");

    // 2. 重新打开数据库并启用 rollover
    drop(tsdb);
    let mut tsdb = TSDBBuilder::file("rollover_test", path, 16 * 1024, 256).open()?;
    tsdb.set_rollover(true); // 启用 rollover
    
    // 清理数据库以进行可预测的测试
    tsdb.reset()?;
    
    // 大量写入，超过数据库容量，以触发翻转
    let mut final_timestamps = Vec::new();
    for i in 1..=100 {
        let ts = i as i64;
        tsdb.append_with_timestamp(ts, &entry_data)?;
        if i > 50 { // 记录后50个时间戳
             final_timestamps.push(ts);
        }
    }

    // 3. 验证翻转后，旧数据被覆盖，新数据存在
    let mut retrieved_timestamps = Vec::new();
    tsdb.tsdb_iter(
        |_, tsl| {
            retrieved_timestamps.push(tsl.time);
            true
        },
        false, // 正向迭代
    );

    // 验证最早的日志已经被删除了
    assert!(!retrieved_timestamps.contains(&1i64), "The oldest data should be rolled over");
    // 验证最新的日志仍然存在
    let last_written_ts = *final_timestamps.last().unwrap();
    assert!(retrieved_timestamps.contains(&last_written_ts), "The newest data should exist");
    
    Ok(())
}


#[test]
fn test_tsdb_edge_cases() -> Result<()> {
    let temp_dir = TempDir::new()?;
    let path = temp_dir.path().to_str().unwrap();
    let max_len = 256;
    let mut tsdb = TSDBBuilder::file("tsdb_edge_test", path, 16 * 1024, max_len).open()?;

    // 1. 时间戳非单调递增
    tsdb.append_with_timestamp(100, b"data1")?;
    // 尝试插入一个更早的时间戳，应该失败
    let result = tsdb.append_with_timestamp(99, b"data_invalid");
    assert!(result.is_err(), "Appending with a non-monotonic timestamp should fail");

    // 2. 数据大小边界
    // a. 空数据
    tsdb.append_with_timestamp(101, b"")?;
    // b. 正好是最大长度
    let max_data = vec![0u8; max_len];
    tsdb.append_with_timestamp(102, &max_data)?;
    // c. 超过最大长度
    let oversized_data = vec![0u8; max_len + 1];
    let result = tsdb.append_with_timestamp(103, &oversized_data);
    assert!(result.is_err(), "Appending data larger than max_len should fail");

    // 3. 验证数据
    let mut count = 0;
    let mut found_empty = false;
    let mut found_max_len = false;
    tsdb.tsdb_iter_by_time(101, 102, |db, tsl| {
        count += 1;
        let value = db.get_value(tsl).unwrap().unwrap();
        if tsl.time == 101 {
            assert_eq!(value.len(), 0);
            found_empty = true;
        }
        if tsl.time == 102 {
            assert_eq!(value.len(), max_len);
            found_max_len = true;
        }
        true
    });
    assert_eq!(count, 2);
    assert!(found_empty && found_max_len);

    Ok(())
}


// 数据读取器测试：验证Read和Seek接口
#[test]
fn test_tsdb_reader_operations() -> Result<()> {
    let temp_dir = TempDir::new()?;
    let path = temp_dir.path().to_str().unwrap();
    let mut tsdb = TSDBBuilder::file("reader_test", path, 32 * 1024, 1024).open()?;

    let test_data = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ";
    let timestamp = 1686451200;
    tsdb.append_with_timestamp(timestamp, test_data)?;

    let mut target_tsl = fdb_tsl::default();
    tsdb.tsdb_iter(
        |_, tsl| {
            target_tsl = *tsl;
            false
        },
        false,
    );

    let mut reader = tsdb.open_read(target_tsl);
    let mut buffer = [0; 10];
    let read_len = reader.read(&mut buffer)?;
    assert_eq!(read_len, 10);
    assert_eq!(&buffer[..read_len], &test_data[0..10]);

    reader.seek(embedded_io::SeekFrom::Start(10))?;
    let mut buffer2 = [0; 10];
    let read_len2 = reader.read(&mut buffer2)?;
    assert_eq!(read_len2, 10);
    assert_eq!(&buffer2[..read_len2], &test_data[10..20]);
    
    reader.seek(embedded_io::SeekFrom::End(-5))?;
    let mut buffer3 = [0; 5];
    let read_len3 = reader.read(&mut buffer3)?;
    assert_eq!(read_len3, 5);
    assert_eq!(&buffer3[..read_len3], &test_data[21..26]);

    Ok(())
}

// 状态管理测试：验证TSL状态转换
#[test]
fn test_tsl_status_management() -> Result<()> {
    let temp_dir = TempDir::new()?;
    let path = temp_dir.path().to_str().unwrap();
    let mut tsdb = TSDBBuilder::file("status_test", path, 16 * 1024, 1024).open()?;

    let test_data = b"Status management test";
    let timestamp = 1686451200;
    tsdb.append_with_timestamp(timestamp, test_data)?;

    let mut target_tsl = Default::default();
    tsdb.tsdb_iter(|_, tsl| {
        target_tsl = tsl.clone();
        false
    }, false);
    assert_eq!(Into::<TSLStatus>::into(target_tsl.status), TSLStatus::Write);

    // 设置为 UserStatus1
    tsdb.set_status(&mut target_tsl, TSLStatus::UserStatus1)?;
    let mut status_checked = false;
    tsdb.tsdb_iter(|_, tsl| {
        if tsl.time == timestamp {
             assert_eq!(Into::<TSLStatus>::into(tsl.status), TSLStatus::UserStatus1);
             status_checked = true;
        }
        true
    }, false);
    assert!(status_checked);
    
    // 设置为 Deleted
    tsdb.set_status(&mut target_tsl, TSLStatus::Deleted)?;
    let mut deleted_checked = false;
    tsdb.tsdb_iter(|db, tsl| {
        if tsl.time == timestamp {
            assert_eq!(Into::<TSLStatus>::into(tsl.status), TSLStatus::Deleted);
            let data = db.get_value(tsl).unwrap();
            assert!(data.is_none(), "Deleted status data should be unreadable");
            deleted_checked = true;
        }
        true
    }, false);
    assert!(deleted_checked);

    Ok(())
}