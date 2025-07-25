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
    // 创建临时目录
    let temp_dir = TempDir::new()?;
    let path = temp_dir.path().to_str().unwrap();

    // 初始化TSDB（128KB最大文件，单条目最大8字节）
    let mut tsdb = TSDBBuilder::file("basic_test", path, 128 * 1024, 1024)
        // .with_sec_size(4096)
        .open()?;

    // 测试数据
    let test_data = b"Test data for TSDB";
    let timestamp = 1686451200_000; // 2023-06-11 00:00:00

    // 1. 追加数据
    tsdb.append_with_timestamp(timestamp, test_data)?;
    
    let timestamp = 1686451201_000; // 2023-06-11 00:00:01
    tsdb.append_with_timestamp(timestamp, test_data)?;

    let count_num = tsdb.count(0, i64::MAX, TSLStatus::Write);
    assert_eq!(count_num, 2, "数据追加后应能通过迭代查询到");

    // 2. 迭代验证数据
    let mut target_tsl = Default::default();
    tsdb.tsdb_iter(
        |db, tsl| {
            // 读取数据并验证
            let data = db.get_value(tsl).unwrap().unwrap();
            assert_eq!(data, test_data);

            target_tsl = tsl.clone();
            false
        },
        false,
    );

    // 3. 按时间查询
    let mut time_count = 0;
    tsdb.tsdb_iter_by_time(timestamp - 100, timestamp + 100, |_, _| {
        time_count += 1;
        true
    });
    assert_eq!(time_count, 1, "时间范围查询应返回匹配数据");

    // 4. 状态设置与验证
    tsdb.set_status(&mut target_tsl, TSLStatus::UserStatus1)?;
    let mut status_checked = false;
    tsdb.tsdb_iter(
        |db, tsl| {
            let tsl_obj = tsl;
            let status: TSLStatus = tsl_obj.status.into();
            assert_eq!(status, TSLStatus::UserStatus1);
            status_checked = true;
            false
        },
        false,
    );
    assert!(status_checked, "状态设置应生效");

    Ok(())
}

// // 边界条件测试：最大容量与rollover功能
// #[test]
// fn test_tsdb_rollover_and_capacity() -> Result<()> {
//     let temp_dir = TempDir::new()?;
//     let path = temp_dir.path().to_str().unwrap();
//
//     // 配置小容量数据库（16KB）并启用rollover
//     let mut tsdb = TSDBBuilder::file("rollover_test", path, 16 * 1024, 1024).open()?;
//     // 1. 测试超出容量时的行为（禁用rollover）
//     tsdb.set_rollover(true);
//     for i in 1..21 {
//         let entry = format!("Entry {}", i);
//         let data = entry.as_bytes();
//         match tsdb.append_with_timestamp(i as i64, data) {
//             Ok(_) if i < 21 => (),         // 前15条应成功
//             Err(_) if i >= 32 => continue, // 后续应失败
//             _ => panic!("容量控制异常"),
//         }
//     }
//
//     // 2. 启用rollover并验证循环写入
//     tsdb.set_rollover(false); // 这里实际启用（参数是禁用标志）
//     let mut rollover_data = Vec::new();
//     for i in 0..30 {
//         let rollover = format!("Rollover {}", i);
//         let data = rollover.as_bytes();
//         tsdb.append_with_timestamp(i as i64, data)?;
//         rollover_data.push(data.to_vec());
//     }
//
//     // 验证最新数据存在（旧数据可能被覆盖）
//     let mut latest_count = 0;
//     tsdb.tsdb_iter(
//         |_, tsl| {
//             if latest_count < 10 {
//                 // 只检查最后10条
//                 latest_count += 1;
//                 true
//             } else {
//                 false // 提前终止
//             }
//         },
//         true, // 反向迭代（最新优先）
//     );
//     assert!(latest_count >= 10, "rollover后应能写入新数据");
//
//     Ok(())
// }

// 数据读取器测试：验证Read和Seek接口
#[test]
fn test_tsdb_reader_operations() -> Result<()> {
    let temp_dir = TempDir::new()?;
    let path = temp_dir.path().to_str().unwrap();

    let mut tsdb = TSDBBuilder::file("reader_test", path, 32 * 1024, 1024).open()?;

    // 写入测试数据
    let test_data = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ";
    let timestamp = 1686451200_000;
    tsdb.append_with_timestamp(timestamp, test_data)?;

    // 获取TSL对象（通过迭代）
    let mut target_tsl = fdb_tsl::default();
    tsdb.tsdb_iter(
        |_, tsl| {
            target_tsl = *tsl;
            false // 找到后终止迭代
        },
        false,
    );

    // 1. 测试Read接口
    let mut reader = tsdb.open_read(target_tsl);
    let mut buffer = [0; 10];

    // 读取前10字节
    let read_len = reader.read(&mut buffer)?;
    assert_eq!(read_len, 10);
    assert_eq!(&buffer[..read_len], &test_data[0..10]);

    // 2. 测试Seek接口（定位到第10字节）
    reader.seek(embedded_io::SeekFrom::Start(10))?;
    let mut buffer2 = [0; 10];
    let read_len2 = reader.read(&mut buffer2)?;
    assert_eq!(read_len2, 10);
    assert_eq!(&buffer2[..read_len2], &test_data[10..20]);

    // 3. 测试SeekFrom::End
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

    // 写入数据并获取时间戳
    let test_data = b"Status management test";
    let timestamp = 1686451200_000;
    tsdb.append_with_timestamp(timestamp, test_data)?;

    // 1. 验证初始状态为Write
    let mut target_tsl = Default::default();
    let mut initial_status = TSLStatus::UNUSED;
    tsdb.tsdb_iter(
        |db, tsl| {
            initial_status = unsafe { core::mem::transmute(tsl.status) };
            target_tsl = tsl.clone();
            false
        },
        false,
    );
    assert_eq!(initial_status, TSLStatus::Write, "初始状态应为Write");

    // 2. 设置为UserStatus1
    tsdb.set_status(&mut target_tsl, TSLStatus::UserStatus1)?;
    let mut status1_checked = false;
    tsdb.tsdb_iter(
        |db, tsl| {
            let status: TSLStatus = tsl.status.into();
            assert_eq!(status, TSLStatus::UserStatus1);
            status1_checked = true;
            false
        },
        false,
    );
    assert!(status1_checked, "UserStatus1设置应生效");

    // 3. 设置为Deleted
    tsdb.set_status(&mut target_tsl, TSLStatus::Deleted)?;
    let mut deleted_checked = false;
    tsdb.tsdb_iter(
        |db, tsl| {
            let status: TSLStatus = tsl.status.into();
            assert_eq!(status, TSLStatus::Deleted);
            deleted_checked = true;

            // 验证Deleted状态数据不可读
            let data = db.get_value(tsl).unwrap();
            assert!(data.is_none(), "Deleted状态数据应不可读");
            false
        },
        false,
    );
    assert!(deleted_checked, "Deleted状态设置应生效");

    Ok(())
}
