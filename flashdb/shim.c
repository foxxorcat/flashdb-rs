#include <stdarg.h>
#include <stdio.h>

// 声明 Rust 函数（在 Rust 中实现）
extern void rust_log(const char *message);

// 实现 fdb_log_printf：将可变参数格式化为字符串，调用 Rust 函数
void fdb_log_printf(const char *format, ...) {
        char buffer[256];
        va_list args;
        va_start(args, format);
        vsnprintf(buffer, sizeof(buffer), format, args);
        va_end(args);
    
        rust_log(buffer); // 传递给 Rust
}