# 达梦 DPI 绑定

`src/ffi.rs` 由 bindgen 从达梦官方头文件生成，生成后提交到仓库，平时编译不需要头文件和 libclang。
达梦头文件（`DPI.h`、`DPIext.h`、`DPItypes.h`）不提交，来自达梦客户端的 `DM_HOME/include`
（Windows 安装包里的 `drivers/dpi/include` 是同一份）。

达梦客户端升级、需要新的 DPI 函数时，按下面的命令重新生成（需要 LLVM 和 `cargo install bindgen-cli --version 0.73.2 --locked`）：

```bash
bindgen crates/dblens-driver-dameng/dpi/wrapper.h \
  --dynamic-loading DmDpi \
  --allowlist-function "dpi_.*" \
  --allowlist-var "DSQL_.*|PG_.*" \
  --no-layout-tests \
  --raw-line "#![allow(non_upper_case_globals, non_camel_case_types, non_snake_case, dead_code, unsafe_op_in_unsafe_fn, clippy::all)]" \
  -o crates/dblens-driver-dameng/src/ffi.rs \
  -- -I "$DM_HOME/include"
cargo fmt --all
```

说明：

- PowerShell 下把 `$DM_HOME` 写成 `$env:DM_HOME`，续行符改为反引号或写成一行。
- 找不到 libclang 时设置 `LIBCLANG_PATH`（例如 `C:\Program Files\LLVM\bin`）。
- 不需要额外的 clang 参数：bindgen 默认按当前 Rust 目标（`x86_64-pc-windows-msvc`）调用 clang，
  clang 自带 `_WIN32`/`_WIN64`，头文件据此定义 `DM64`，`slength`/`ulength` 生成为 64 位整数。
  在 64 位 Linux 上生成时，头文件通过 `bits/wordsize.h` 得到同样的结果。
- `unsafe_op_in_unsafe_fn`：bindgen 0.73 生成的动态加载代码在 `unsafe fn` 内直接调用 unsafe 函数，
  本仓库使用 2024 edition，不加这一项会产生大量警告（`--rust-edition 2024` 也不会改变这部分生成代码）。
- 最后的 `cargo fmt --all` 让生成文件符合仓库的 `rustfmt.toml`，否则 CI 的格式检查不通过。
- 生成的方法都是 `unsafe fn`；DLL 中缺少某个函数时，生成的加载代码本身仍会成功，调用该方法时才 panic。
  驱动的 `load.rs` 在加载后立即按 `required_functions!` 列表逐个校验这些函数，缺失时报告 `DriverUnavailable`，
  而不是等到调用时 panic。因此在驱动中新增任何 DPI 调用时，都必须把对应函数加入 `required_functions!` 列表。

生成所用的达梦客户端版本记录在 `docs/notes/dameng-dpi-spike.md`。
