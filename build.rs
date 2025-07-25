extern crate bindgen;

use std::env;
use std::path::PathBuf;

fn main() {    
    let srcs = [
        "flashdb/fdb.c",
        "flashdb/fdb_file.c",
        "flashdb/fdb_kvdb.c",
        "flashdb/fdb_tsdb.c",
        "flashdb/fdb_utils.c",
        "flashdb/shim.c",
    ];

    // 定义统一的配置选项
    let log_tag = "\"flashdb-rs\"";

    let file_mode = if cfg!(target_os = "windows") {
        "FDB_USING_FILE_LIBC_MODE"
    } else {
        "FDB_USING_FILE_POSIX_MODE"
    };
    let use_64bit_timestamp = cfg!(feature = "time64");
    let use_kvdb = cfg!(feature = "kvdb");
    let use_tsdb = cfg!(feature = "tsdb");
    let debug_enabled = cfg!(debug_assertions);

    // 编译 FlashDB C 库
    let mut build = cc::Build::new();
    build
        .files(&srcs)
        .include("flashdb/inc")
        // .compiler("xtensa-esp32s3-elf-gcc")
        .flag_if_supported("-mlongcalls")
        .flag_if_supported("-Wno-macro-redefined")
        .std("c99")
        .warnings(false);

    // 应用配置到编译过程
    if use_64bit_timestamp {
        build.define("FDB_USING_TIMESTAMP_64BIT", "1");
    }
    build.define("FDB_LOG_TAG", log_tag);
    build.define(file_mode, "1");

    if use_kvdb {
        build.define("FDB_USING_KVDB", "1");
    }
    if use_tsdb {
        build.define("FDB_USING_TSDB", "1");
    }
    if debug_enabled {
        build.define("FDB_DEBUG_ENABLE", "1");
    }

    build.compile("flashdb");

    // 生成 Rust 绑定
    let mut bindings = bindgen::Builder::default()
        .header("flashdb/inc/flashdb.h")
        .parse_callbacks(Box::new(bindgen::CargoCallbacks::new()))
        .allowlist_type("fdb_.*")
        .allowlist_function("fdb_.*")
        .allowlist_var("FDB_.*")
        .use_core()
        .derive_default(true)
        .derive_debug(true);

    // 应用相同的配置到绑定生成过程
    if use_64bit_timestamp {
        bindings = bindings.clang_arg("-DFDB_USING_TIMESTAMP_64BIT=1");
    }
    bindings = bindings.clang_arg(format!("-DFDB_LOG_TAG={}", log_tag));
    bindings = bindings.clang_arg(format!("-D{}=1", file_mode));

    if use_kvdb {
        bindings = bindings.clang_arg("-DFDB_USING_KVDB=1");
    }
    if use_tsdb {
        bindings = bindings.clang_arg("-DFDB_USING_TSDB=1");
    }
    if debug_enabled {
        bindings = bindings.clang_arg("-DFDB_DEBUG_ENABLE=1");
    }

    let bindings = bindings.generate().expect("Unable to generate bindings");

    // 输出绑定文件
    let out_path = PathBuf::from(env::var("OUT_DIR").unwrap());
    bindings
        .write_to_file(out_path.join("bindings.rs"))
        .expect("Couldn't write bindings!");
}
