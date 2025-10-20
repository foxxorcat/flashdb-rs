use std::env;
use std::path::{Path, PathBuf};
use std::process::Command;

// 回退函数：当 `-print-sysroot` 失败时，通过解析编译器输出来获取头文件搜索路径
fn get_compiler_include_paths(compiler_path: &Path) -> Result<Vec<String>, std::io::Error> {
    println!(
        "cargo:info=Sysroot 自动检测失败，回退至解析编译器 include 路径模式。"
    );
    // 构造一个命令，让编译器预处理一个空输入，并打印出头文件搜索路径
    // `arm-none-eabi-gcc -E -Wp,-v -xc /dev/null`
    // 在 Windows 上，`/dev/null` 应该替换为 `NUL`
    let null_device = if cfg!(windows) { "NUL" } else { "/dev/null" };

    // 执行命令，捕获编译器的 stderr 输出
    let output = Command::new(compiler_path)
        .arg("-E")
        .arg("-Wp,-v")
        .arg("-xc")
        .arg(null_device)
        .output()?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        println!(
            "cargo:warning=从编译器获取 include 路径失败。 Stderr:\n{}",
            stderr
        );
        // 返回空 Vec，让后续流程决定是否因缺少路径而构建失败
        return Ok(Vec::new());
    }

    let stderr = String::from_utf8_lossy(&output.stderr);
    let mut clang_args = Vec::new();
    let mut in_search_list = false;

    // 解析 stderr，提取 #include <...> 搜索路径
    for line in stderr.lines() {
        if line.starts_with("#include <...> search starts here:") {
            in_search_list = true;
            continue;
        }
        if line.starts_with("End of search list.") {
            break;
        }
        if in_search_list {
            // 将路径格式化为 clang 的 -I 参数
            clang_args.push(format!("-I{}", line.trim()));
        }
    }

    if clang_args.is_empty() {
        println!(
            "cargo:warning=无法自动确定编译器的 include 路径。 Stderr:\n{}",
            stderr
        );
    } else {
        println!(
            "cargo:info=成功解析到编译器 include 路径: {:?}",
            clang_args
        );
    }

    Ok(clang_args)
}

fn main() {
    let target = env::var("TARGET").unwrap();
    let out_path = PathBuf::from(env::var("OUT_DIR").unwrap());

    let mut build = cc::Build::new();
    let mut bindings = bindgen::Builder::default();

    let compiler = build.get_compiler();
    let compiler_path = compiler.path();

    // 方案一：尝试直接获取 sysroot
    let sysroot_output = Command::new(compiler_path).arg("-print-sysroot").output();

    let mut found_headers = false;
    match sysroot_output {
        Ok(output) if output.status.success() => {
            let sysroot_path = String::from_utf8_lossy(&output.stdout).trim().to_string();
            if !sysroot_path.is_empty() {
                println!(
                    "cargo:info=为目标 {} 自动检测到 sysroot: {}",
                    target, sysroot_path
                );
                let sysroot_arg = format!("--sysroot={}", sysroot_path);

                // 同时应用到 cc::Build 和 bindgen::Builder
                build.flag(&sysroot_arg);
                bindings = bindings.clang_arg(&sysroot_arg);
                found_headers = true;
            }
        }
        _ => { // 如果失败，则静默处理，后续将进入回退方案
        }
    }

    // 方案二：如果 sysroot 获取失败，则回退到解析 include 路径的方案
    if !found_headers {
        match get_compiler_include_paths(compiler_path) {
            Ok(include_args) => {
                for arg in include_args {
                    // 同样，同时应用到 cc::Build 和 bindgen::Builder
                    build.flag(arg.as_str());
                    bindings = bindings.clang_arg(arg);
                }
            }
            Err(e) => {
                println!(
                    "cargo:warning=执行编译器以获取 include 路径失败。错误: {}",
                    e
                );
            }
        }
    }
    // --- 头文件路径检测逻辑结束 ---

    let mut srcs = vec![
        "flashdb/fdb.c",
        "flashdb/fdb_kvdb.c",
        "flashdb/fdb_tsdb.c",
        "flashdb/fdb_utils.c",
    ];

    let use_64bit_timestamp = cfg!(feature = "time64");
    let use_kvdb = cfg!(feature = "kvdb");
    let use_tsdb = cfg!(feature = "tsdb");
    let use_log = cfg!(feature = "log");
    let debug_enabled = cfg!(debug_assertions);

    // --- 写入粒度 (FDB_WRITE_GRAN) 配置 ---
    let mut gran_features = Vec::new();
    let mut gran_value = "1"; // 初始化为空

    if cfg!(feature = "gran-1") { gran_features.push("gran-1"); gran_value = "1"; }
    if cfg!(feature = "gran-8") { gran_features.push("gran-8"); gran_value = "8"; }
    if cfg!(feature = "gran-32") { gran_features.push("gran-32"); gran_value = "32"; }
    if cfg!(feature = "gran-64") { gran_features.push("gran-64"); gran_value = "64"; }
    if cfg!(feature = "gran-128") { gran_features.push("gran-128"); gran_value = "128"; }

    // 互斥检查：确保只选择了一个 gran 特性
    if gran_features.len() > 1 {
        panic!("错误：只能选择一个 'gran-x' 特性，但检测到多个: {:?}", gran_features);
    }

    // 强制检查：必须选择一个 gran 特性
    if gran_features.is_empty() {
        println!("cargo:warning=未指定 'gran-x' 特性，将默认使用 FDB_WRITE_GRAN=1 (适用于 NOR Flash)。如果使用 STM32 内部 Flash，这可能会导致错误。");
    }

    // 如果检查通过，gran_value 必定已被正确设置
    println!("cargo:info=已选择 FlashDB 写入粒度: {} bits", gran_value);
    build.define("FDB_WRITE_GRAN", Some(gran_value));
    bindings = bindings.clang_arg(format!("-DFDB_WRITE_GRAN={}", gran_value));

    {
        let linker = match target.as_str() {
            "xtensa-esp32-espidf" => Some("xtensa-esp32-elf-gcc"),
            "xtensa-esp32s2-espidf" => Some("xtensa-esp32s2-elf-gcc"),
            "xtensa-esp32s3-espidf" => Some("xtensa-esp32s3-elf-gcc"),

            // cc 中有相关映射，应该可以不设置
            // Keep C3 as the first in the list, so it is picked up by default; as C2 does not work for older ESP IDFs
            "riscv32imc-esp-espidf" |
            // Keep C6 at the first in the list, so it is picked up by default; as H2 does not have a Wifi
            "riscv32imac-esp-espidf" | "riscv32imafc-esp-espidf" => Some("riscv32-esp-elf-gcc"),
            _ => None
        };

        if let Some(linker) = linker {
            build.flag_if_supported("-mlongcalls");
            build.compiler(linker);
        }
    }

    // 根据 log 特性决定日志实现
    if use_log {
        // 编译 shim.c 并重定向 FDB_PRINT 到 Rust
        srcs.push("flashdb/shim.c");
        build.define("FDB_PRINT(...)", "fdb_log_printf(__VA_ARGS__)");
    } else {
        // 不启用 log 时，将 FDB_PRINT 定义为空操作，移除 stdio 依赖
        build.define("FDB_PRINT(...)", "((void)0)");
    }

    build
        .files(&srcs)
        .include("flashdb/inc")
        .cargo_warnings(false);

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
    bindings = bindings
        .use_core()
        .header("flashdb/inc/flashdb.h")
        .parse_callbacks(Box::new(bindgen::CargoCallbacks::new()))
        .allowlist_type("fdb_.*")
        .allowlist_function("fdb_.*")
        .allowlist_var("FDB_.*")
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
    if !use_log {
        bindings = bindings.clang_arg("-DFDB_PRINT(...)=");
    }

    let bindings = bindings.generate().expect("Unable to generate bindings");

    // 输出绑定文件
    bindings
        .write_to_file(out_path.join("bindings.rs"))
        .expect("Couldn't write bindings!");
}