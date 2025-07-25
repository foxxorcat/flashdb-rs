// benches/performance_bench.rs

use criterion::{criterion_group, criterion_main, BatchSize, BenchmarkId, Criterion, Throughput};
use flashdb_rs::{kvdb::KVDBBuilder, tsdb::TSDBBuilder};
use std::path::Path;
use tempfile::tempdir;

// --- 辅助函数 ---

fn setup_kvdb(dir: &Path) -> flashdb_rs::kvdb::KVDB {
    KVDBBuilder::file("kv_bench_db", dir.to_str().unwrap(), 10 * 1024 * 1024) // 10MB 数据库
        .with_sec_size(4096)
        .open()
        .unwrap()
}

fn setup_tsdb(dir: &Path) -> flashdb_rs::tsdb::TSDB {
    TSDBBuilder::file("ts_bench_db", dir.to_str().unwrap(), 10 * 1024 * 1024, 1024)
        .open()
        .unwrap()
}

// --- KVDB 性能测试 ---

/// 测试写入全新的、不重复的键的性能。
fn kvdb_set_benchmark(c: &mut Criterion) {
    let temp_dir = tempdir().unwrap();
    let mut db = setup_kvdb(temp_dir.path());
    let value = vec![0u8; 256]; // 256字节的值

    c.bench_function("kvdb_set_new", |b| {
        let mut i = 0;
        b.iter(|| {
            let key = format!("key_{}", i);
            db.set(&key, &value).unwrap();
            i += 1;
        })
    });
}

/// 测试重复读取同一个键的性能。
fn kvdb_get_benchmark(c: &mut Criterion) {
    let temp_dir = tempdir().unwrap();
    let mut db = setup_kvdb(temp_dir.path());
    let value = vec![0u8; 256];
    let key = "persistent_key";
    db.set(key, &value).unwrap();

    c.bench_function("kvdb_get", |b| {
        b.iter(|| {
            db.get(key).unwrap();
        })
    });
}

/// 测试覆盖写入同一个键的性能。
fn kvdb_overwrite_benchmark(c: &mut Criterion) {
    let temp_dir = tempdir().unwrap();
    let mut db = setup_kvdb(temp_dir.path());
    let initial_value = vec![0u8; 256];
    let overwrite_value = vec![1u8; 256];
    let key = "overwrite_key";
    db.set(key, &initial_value).unwrap();

    c.bench_function("kvdb_set_overwrite", |b| {
        b.iter(|| {
            db.set(key, &overwrite_value).unwrap();
        })
    });
}

// --- TSDB 性能测试 ---

/// **[已修复]** 测试向一个全新的数据库追加单条TSL的性能。
fn tsdb_append_benchmark(c: &mut Criterion) {
    let log_data = vec![0u8; 256];

    let mut group = c.benchmark_group("tsdb_append");
    group.throughput(Throughput::Bytes(log_data.len() as u64)); // 按字节报告吞吐量
    group.bench_function("tsdb_append_256B", |b| {
        b.iter_batched(
            // 设置 (Setup): 这会在每次运行测试例程前创建一个全新的数据库实例。
            || {
                let temp_dir = tempdir().unwrap();
                let db = setup_tsdb(temp_dir.path());
                // 同时返回 temp_dir 和 db，以确保 temp_dir 在测试期间保持存活。
                (temp_dir, db)
            },
            // 测试例程 (Routine): 这是被计时的代码。它接收上面 setup 返回的全新状态。
            |(_temp_dir, mut db)| {
                // 因为数据库是全新的，它的 last_time 是 0。所以我们总能成功追加时间戳为 1 的日志。
                db.append_with_timestamp(1, &log_data).unwrap();
            },
            // BatchSize::PerIteration 会在每次计时的迭代前都运行 setup。
            // 这可以防止一次迭代的状态影响下一次迭代。
            BatchSize::PerIteration,
        )
    });
    group.finish();
}

/// 测试在一个预填充好数据的数据库上进行查询的性能。
fn tsdb_query_benchmark(c: &mut Criterion) {
    let temp_dir = tempdir().unwrap();
    let mut db = setup_tsdb(temp_dir.path());
    let log_data = vec![0u8; 64];

    // 预先填充大量数据。
    for i in 1..=10000 {
        db.append_with_timestamp(i, &log_data).unwrap();
    }

    // 测试查询不同数量记录的性能。
    for &query_size in &[1, 10, 100, 1000] {
        c.bench_with_input(
            BenchmarkId::new("tsdb_iter_by_time", query_size),
            &query_size,
            |b, &size| {
                b.iter(|| {
                    let mut count = 0;
                    // 从数据集的中间查询一个固定的时间范围。
                    db.tsdb_iter_by_time(5000, 5000 + size - 1, |_db, _tsl| {
                        count += 1;
                        true // 继续迭代
                    });
                    assert_eq!(count, size);
                })
            },
        );
    }
}

// 注册所有性能测试
criterion_group!(
    benches,
    kvdb_set_benchmark,
    kvdb_get_benchmark,
    kvdb_overwrite_benchmark,
    tsdb_append_benchmark,
    tsdb_query_benchmark
);
criterion_main!(benches);
