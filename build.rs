use std::env;
use std::path::PathBuf;

fn main() {
    let target = env::var("TARGET").expect("TARGET environment variable not set");

    let srcs = [
        "flashdb/fdb.c",
        "flashdb/fdb_file.c",
        "flashdb/fdb_kvdb.c",
        "flashdb/fdb_tsdb.c",
        "flashdb/fdb_utils.c",
        "flashdb/shim.c",
    ];

    let use_64bit_timestamp = cfg!(feature = "time64");
    let use_kvdb = cfg!(feature = "kvdb");
    let use_tsdb = cfg!(feature = "tsdb");
    let use_log = cfg!(feature = "log");
    let debug_enabled = cfg!(debug_assertions);

    // 编译 FlashDB C 库
    let mut build = cc::Build::new();

    {
        let linker = match target.as_str() {
            "xtensa-esp32-espidf" =>Some("xtensa-esp32-elf-gcc"),
            "xtensa-esp32s2-espidf" =>Some("xtensa-esp32s2-elf-gcc"),
            "xtensa-esp32s3-espidf" => Some("xtensa-esp32s3-elf-gcc"),

            // cc 中有相关映射，应该可以不设置
            // Keep C3 as the first in the list, so it is picked up by default; as C2 does not work for older ESP IDFs
            "riscv32imc-esp-espidf"|
            // Keep C6 at the first in the list, so it is picked up by default; as H2 does not have a Wifi
            "riscv32imac-esp-espidf" | "riscv32imafc-esp-espidf" => Some("riscv32-esp-elf-gcc"),
            _ => None
        };

        if let Some(linker) = linker {
            build.flag_if_supported("-mlongcalls");
            build.compiler(linker);
        }
    }

    build
        .files(&srcs)
        .include("flashdb/inc")
        .cargo_warnings(false);

    // 应用配置到编译过程
    // 将日志打印转发到rust处理
    if use_log {
        build.define("FDB_PRINT(...)", "fdb_log_printf(__VA_ARGS__)");
    }

    build.define("FDB_USING_CUSTOM_MODE", "1");

    if use_64bit_timestamp {
        build.define("FDB_USING_TIMESTAMP_64BIT", "1");
    }

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

    println!("cargo:rerun-if-changed=flashdb/inc/flashdb.h");
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
    bindings = bindings.clang_arg("-DFDB_USING_CUSTOM_MODE=1");

    if use_64bit_timestamp {
        bindings = bindings.clang_arg("-DFDB_USING_TIMESTAMP_64BIT=1");
    }

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
