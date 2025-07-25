# flashdb-rs

这是一个为 [FlashDB](https://github.com/armink/FlashDB) 编写的安全、高性能的 Rust 绑定库。`FlashDB` 是一款专注于嵌入式产品的超轻量级数据库。

本库旨在为 Rust 开发者提供一个安全且符合人体工程学的接口，以便在 Rust 项目中（包括 `no_std` 环境）无缝使用 `FlashDB` 强大的键值（KV）和时序（TSDB）存储功能，而无需直接编写 `unsafe` 的 C 代码。

## 原始库介绍

[FlashDB](https://github.com/armink/FlashDB) 是一款超轻量级的嵌入式数据库，专注于提供嵌入式产品的数据存储方案。FlashDB 不仅支持传统的基于文件系统的数据库模式，而且结合了 Flash 的特性，具有较强的性能及可靠性，并在保证极低的资源占用前提下，尽可能延长 Flash 使用寿命。

它提供两种数据库模式：

  - **键值数据库 (KVDB)**：将数据存储为键值对集合，操作简洁，可扩展性强。
  - **时序数据库 (TSDB)**：将数据按照时间顺序存储，适用于日志记录、传感器数据等场景，具有高性能的插入和查询能力。

## `flashdb-rs` 的特性

  - **内存安全保证**：通过 Rust 的所有权和生命周期管理，将底层的 C 库接口封装在安全的 API 之后。
  - **符合人体工程学的 API**：使用构建者模式（Builder Pattern）进行数据库初始化，提供 `Result` 进行错误处理，并为数据访问提供了流式读取器（Reader）和迭代器（Iterator）。
  - **灵活的存储后端**：通过 `storage::Storage` trait 将存储层完全抽象。您可以为任何 Flash 硬件（内部 Flash、QSPI、SPI Nor/NAND 等）实现自己的存储后端。
  - **内置文件系统支持**：在 `std` 环境下，提供开箱即用的文件存储后端（`StdStorage`），方便在桌面环境进行开发和测试。
  - **`no_std` 兼容**：专为嵌入式和裸机环境设计，只需实现 `Storage` trait 即可在不同平台上运行。
  - **特性控制（Feature Gates）**：您可以根据需要仅启用 `kvdb` 或 `tsdb` 功能，最大限度地减少固件体积。

## 快速上手

### 1\. 添加依赖

将以下内容添加到您的 `Cargo.toml` 中：

```toml
[dependencies]
flashdb-rs = { version = "0.0.1", features = ["kvdb", "tsdb", "std", "time64"] }

# 用于桌面测试
tempfile = "3.4.0"
```

### 2\. KVDB (键值数据库) 示例

```rust
use flashdb_rs::kvdb::KVDBBuilder;
use tempfile::tempdir;

fn main() -> anyhow::Result<()> {
    // 1. 创建一个临时目录用于存储数据库文件
    let temp_dir = tempdir()?;
    let db_path = temp_dir.path();

    // 2. 使用构建者模式创建并打开数据库
    let mut db = KVDBBuilder::file("kv_db", db_path.to_str().unwrap(), 128 * 1024)
        .with_sec_size(4096)
        .open()?;

    // 3. 设置键值对
    let key = "boot_count";
    let value = "10";
    db.set(key, value.as_bytes())?;
    println!("Set '{}' = '{}'", key, value);

    // 4. 获取键值对
    if let Some(retrieved_value) = db.get(key)? {
        let value_str = std::str::from_utf8(&retrieved_value).unwrap();
        println!("Get '{}' = '{}'", key, value_str);
        assert_eq!(value_str, value);
    }

    // 5. 迭代所有键值对
    for entry_result in db.iter() {
        let entry = entry_result?;
        println!("Found key: {}", entry.key);
    }

    Ok(())
}
```

### 3\. TSDB (时序数据库) 示例

```rust
use flashdb-rs::tsdb::{TSDBBuilder, TSLStatus};
use tempfile::tempdir;

fn main() -> anyhow::Result<()> {
    // 1. 创建临时目录
    let temp_dir = tempdir()?;
    let db_path = temp_dir.path();

    // 2. 初始化 TSDB
    let mut tsdb = TSDBBuilder::file("ts_db", db_path.to_str().unwrap(), 128 * 1024, 1024).open()?;

    // 3. 追加带时间戳的日志
    let timestamp1 = 1686451200; // Unix aarch
    let data1 = b"log entry 1: system started";
    tsdb.append_with_timestamp(timestamp1, data1)?;
    println!("Appended log at timestamp {}", timestamp1);

    let timestamp2 = 1686451260;
    let data2 = b"log entry 2: sensor reading OK";
    tsdb.append_with_timestamp(timestamp2, data2)?;
    println!("Appended log at timestamp {}", timestamp2);

    // 4. 按时间范围迭代日志
    println!("\nIterating logs from {} to {}:", timestamp1, timestamp2);
    tsdb.tsdb_iter_by_time(timestamp1, timestamp2, |db, tsl| {
        if let Ok(Some(value)) = db.get_value(tsl) {
            println!(
                "  - Time: {}, Data: '{}'",
                tsl.time,
                std::str::from_utf8(&value).unwrap()
            );
        }
        true // 返回 true 继续迭代
    });

    Ok(())
}
```

## 在嵌入式 (`no_std`) 环境中使用


1.  **修改 `Cargo.toml`**：
    禁用默认的 `std` 特性。

    ```toml
    [dependencies]
    flashdb-rs = { version = "0.0.1", default-features = false, features = ["kvdb", "time64"] }
    ```

2.  **实现 `Storage` Trait**：
    您需要为您目标平台的 Flash 存储器（例如 STM32 的内部 Flash 或 ESP32 的 SPI Flash）实现 `storage::Storage` trait。这需要您编写读、写和擦除 Flash 的具体逻辑。

    ```rust
    // 伪代码示例
    use flashdb_rs::storage::Storage;
    use flashdb_rs::error::Error;
    use embedded_io::{Read, Write, Seek};

    // 假设这是您为特定硬件实现的存储驱动
    struct MyHardwareFlash;

    // 为您的驱动实现 embedded_io traits...
    impl Read for MyHardwareFlash { /* ... */ }
    impl Write for MyHardwareFlash { /* ... */ }
    impl Seek for MyHardwareFlash { /* ... */ }

    // 实现 flashdb-rs 的 Storage trait
    impl Storage for MyHardwareFlash {
        fn erase(&mut self, addr: u32, size: usize) -> Result<(), Error> {
            // 在这里调用您硬件的扇区擦除函数
            // ...
            Ok(())
        }
    }
    ```

3.  **初始化数据库**：
    使用 `open_with` 方法传入您自定义的 `Storage` 实例。

    ```rust
    use flashdb_rs::kvdb::KVDBBuilder;

    fn my_embedded_main() {
        let my_flash_storage = MyHardwareFlash; // 创建您的存储实例

        let mut db = KVDBBuilder::new("config", 64 * 1024)
            .with_sec_size(4096)
            .open_with(my_flash_storage)
            .expect("Failed to open db");
        
        // 现在可以正常使用 db
        db.set("wifi_ssid", b"MyNetwork").unwrap();
    }
    ```

## 许可证

本项目采用 **Apache-2.0** 开源协议。

## 致谢

  - 感谢 [armink](https://github.com/armink) 开发了如此出色的 `FlashDB` C 库。
  - 本项目依赖于原始的 [FlashDB 仓库](https://github.com/armink/FlashDB)。