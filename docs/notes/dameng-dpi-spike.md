# 达梦 DPI 技术验证结论（日期：2026-10-22）

> 2026-10-31：原生 DPI 驱动已移除，达梦统一走 JDBC 桥接。这篇只作历史记录。

- 达梦客户端版本（生成 ffi.rs 所用的头文件）：DM8 Windows 安装包（2026-07-09 构建）中的 `include/DPI.h`、`DPIext.h`、`DPItypes.h`；
  运行时 `drivers/dpi/dmdpi.dll` 内嵌版本串 `V8.1.5.60-Build(2026.07.07-335949)ENT`（DLL 没有 Windows 版本资源）
- 测试库：`DM Database Server 64 V8`（V$VERSION 其余行：`DB Version: 0x7000d`、`03134284604-20260707-335949-20228`、`Msg Version: 6`、`Gsu level(5) cnt: 102`）；
  库字符集 UTF-8（`UNICODE()` = 1），`LENGTH_IN_CHAR` = 0
- 客户端平台：Windows x64（`x86_64-pc-windows-msvc`），bindgen-cli 0.73.2 + LLVM 23.1.4

| 验证项 | 结果 | 观察 |
|---|---|---|
| 连接与中文（PG_UTF8） | 通过 | 登录前设置 `DSQL_ATTR_LOCAL_CODE = PG_UTF8`、`DSQL_ATTR_LOGIN_PORT`，`dpi_login(主机, 用户, 密码)` 成功。`SELECT '数洞', 1, SYSDATE FROM DUAL` 的列描述：`'数洞'` 类型 2（VARCHAR）精度 6、`1` 类型 7（INT）精度 10、`SYSDATE` 类型 16（DSQL_TIMESTAMP）精度 19，均可空；列名、数据、错误信息都是 UTF-8，"数洞" 原样往返 |
| 从其他线程 dpi_cancel | 通过 | 计划原用的三表笛卡尔积 COUNT 会被优化成计数相乘，约 0.12 秒就返回，取消请求到达时语句已结束（第一次运行因此"失败"）。换成逐行计算条件的查询（不取消约 132 秒）后：`dpi_cancel` 返回 0（DSQL_SUCCESS），调用本身约 6 ms；被取消的 `dpi_exec_direct` 返回错误，诊断码 **-6515**「操作被取消」；从发出取消到语句返回约 **3～3.5 秒**（200 ms / 500 ms / 1000 ms / 2500 ms 后取消，语句分别在约 4.0 / 3.1～3.7 / 4.0 / 6.0 秒返回），PL/SQL `WHILE` 循环同样可取消、延迟相同；取消后连接可继续使用，同一语句句柄也可直接复用 |
| CLOB 分块读取 | 通过 | `SELECT RPAD(TO_CLOB('a'), 100000, 'a') FROM DUAL` 可用，列类型 19（DSQL_CLOB）精度 2147483647。`DSQL_C_NCHAR` 读取时，`dpi_get_data` 的长度输出是**本次调用开始时剩余的字节数（客户端编码 UTF-8，不含结尾 \0）**：5 万个汉字的 CLOB 第一次报告 150000（字节，不是字符；`DBMS_LOB.GETLENGTH` 为 50000），之后依次 84465、18930。没有 NO_TOTAL（头文件里也没有这个常量）。缓冲区写满时返回 1（SUCCESS_WITH_INFO），最后一块返回 0。分块会切在 UTF-8 字符中间（1001 字节缓冲区每块 1000 字节），拼接后完整有效 |
| 错误码格式 | 通过 | `dpi_get_diag_rec` 给出负数错误码和中文信息：不存在的表 **-2106**「第1 行附近出现错误:\n无效的表或视图名[DBLENS_NO_SUCH_TABLE]」；语法错误 **-2007**「第 1 行, 第 6 列[1]附近出现错误: \n语法分析出错」 |

补充验证：

- Windows 上用 `LOAD_WITH_ALTERED_SEARCH_PATH` 加载 `dmdpi.dll`，其依赖（dmcalc.dll 等）从同一目录加载，不需要把目录加入 PATH；
  不带该标志直接 `LoadLibrary` 失败（错误 126）。该标志要求路径使用反斜杠：用正斜杠的路径（如 `E:/dmdbms/drivers/dpi/dmdpi.dll`）同样报 126，
  因此 `load.rs` 在加载前把 `/` 统一替换成 `\`（`spike_5` 验证）。`drivers\dpi\dependencies\` 下的 OpenSSL/VC++ 运行库不需要。
- 生成的 ffi.rs 共 105 个 `dpi_` 函数；计划列出的类型别名、函数和 16 个常量名字都与头文件一致。

## 结论

**继续使用 DPI，不改走 ODBC。** 决定性的两项（连接与中文、跨线程取消）都通过：UTF-8 往返正确，`dpi_cancel` 可以从另一线程中断正在执行的查询和 PL/SQL，
返回明确的错误码，连接和语句句柄在取消后都可继续使用。LOB 分块读取和错误码也符合预期。头文件的函数签名与 dmPython 的调用方式基本一致，
只有一处输出参数类型需要调整（见下）。唯一的不足是取消延迟约 3 秒，看起来是服务器端检查中断的粒度（查询和 PL/SQL 循环的延迟相同、与发出取消的时机无关），改用 ODBC 预计不会改善（未实测）。

## 对 Task 4 的影响

1. **被取消语句的错误码是 -6515**（`Error::Sql { code: "-6515", .. }`）。驱动应在已请求取消且收到 -6515 时映射为 `Error::Cancelled`。
2. **取消延迟约 3～3.5 秒**：`dpi_cancel` 本身立即返回（约 6 ms），语句要再过约 3 秒才结束。界面应显示"正在取消"，取消相关的测试超时不要低于 5 秒（建议 10 秒）。
3. **取消后句柄可复用**：取消后连接和同一个 `dhstmt` 都能继续执行，不需要重建连接。
4. **CLOB/文本长度单位是字节**：`dpi_get_data`（`DSQL_C_NCHAR`）报告的长度是客户端编码（UTF-8）下的剩余字节数，不是字符数；第一次调用的值即总字节数，
   后续调用报告的是剩余字节数，不能当成总长累加。判断预览是否完整、显示"共 N 字符"时，需要另取 `DBMS_LOB.GETLENGTH` 或按字节说明。
5. **分块会切断 UTF-8 字符**：每块的有效字节 = 缓冲区长度 − 1（结尾 \0），不保证落在字符边界。必须先拼接字节再解码；截断预览时要退回到字符边界再 `from_utf8`。
6. **`get_data` 的 `limit` 判断**：文本每块只有 65535 字节，64 KB 的限额会多读一整块（spike_3 中 10 万字节被全部读完）。
   Task 4 最终没有改成"读到 ≥ limit 即停"，而是保留 `out.len() > limit` 的判断，目的是让读到的字节数一旦超过限额，
   后续的 `cap_large` 一定会触发，把值截断在 limit 处（并退回到字符边界）。另外 `bytes_value` 在数据库报告的总长度大于已读字节数时，
   一律把值标记为截断的 LOB 预览（`total_len` 取数据库报告的总长度），所以提前停止读取不会把不完整的内容当作完整值返回。
7. **VARCHAR 的列精度是字节数**（`'数洞'` 精度 6，库为 UTF-8、`LENGTH_IN_CHAR=0`），类型映射时不要把精度当作字符数。
8. **必须在登录前设置 `DSQL_ATTR_LOCAL_CODE = PG_UTF8`**：列名、数据和错误信息因此都是 UTF-8；错误信息中可能带换行和"第 N 行, 第 M 列"的位置描述，
   需要时可从中解析 `position`，并去掉多余的换行。
9. **类型以 ffi.rs 为准**：`dpi_number_columns` 的输出参数是 `sdint2*`（计划写的是 `udint2`）；`dpi_desc_column` 的列号是 `sdint2`，
   `dpi_get_data` 的列号是 `udint2`；`slength`/`ulength` 为 i64/u64；`DPIRETURN` 为 i16；`DSQL_*` 常量生成为 `u32`（`DSQL_NULL_DATA` 为 `i32`），比较时需转换。
   `dpi_fetch` 的输出参数 `row_num`（`ulength*`）按名字是本次取到的行数，本次未使用其值。
10. **Windows DLL 加载**：保留 `LOAD_WITH_ALTERED_SEARCH_PATH`，并保留路径分隔符归一化（用户在设置里填正斜杠目录很常见）。
11. **每个连接使用自己的 `Env`**：`connect_blocking` 为每个连接单独创建环境句柄（`Env`），由对应的 `Con` 直接持有（不再用 `Arc` 共享），连接之间不共享，因此不存在同一 env 上并发 `dpi_alloc_con` 的线程安全问题，`Env` 也不需要 `Sync`（只保留 `Send`，句柄随连接移交给工作线程）。
12. **临时 `allow(dead_code)`**：`lib.rs` 中 `handles`、`load` 模块的 `#[allow(dead_code)]` 在驱动本体接入后去掉。
13. Linux 下 `libdmdpi.so` 的依赖加载（`LD_LIBRARY_PATH`）本次未验证。
14. **影响 0 行的 UPDATE 在 DM 上返回 DSQL_SUCCESS（0），不是 DSQL_NO_DATA**（`exec_direct` 与 `prepare` + `exec` 都实测为 0，`row_count` 为 0，见 `spike_6`）。`Stmt::exec_direct` / `exec` 仍把 NO_DATA 当作成功，以防其他版本或语句类型返回 100；判断"没有影响行"要用 `row_count`，不能看返回值。
