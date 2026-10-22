# M0-2 遗留事项

M0-2（达梦与 Oracle 驱动）在逐任务审查和全分支最终审查中，有一部分问题被判定为不阻塞合并，记录在这里。写 M0-3 / M0-4 / M1 的计划时逐条处理，或明确说明不做。

## M0-4 之前

- 达梦登录失败目前统一报告为 `Connect`，没有细分出 `Auth`。原因是不想用错误密码去试探认证错误码，以免锁住测试账号。另外，达梦驱动没有使用 `ConnectParams.database`（模式）。
- Oracle 连接串：服务名、SID 和主机名都没有转义；IPv6 地址没有加方括号；Easy Connect 的 `connect_timeout` 需要 19c 以上的客户端；不足 1 秒的超时会变成 0。
- 原生库路径：配置里的 `dpi_lib_dir` / `oracle_client_lib_dir`，以及环境变量 `ORACLE_HOME` / `DM_HOME`，如果是相对路径，目前也会被接受。应该在加载前用 `std::path::absolute` 转成绝对路径。另外，`LOAD_WITH_ALTERED_SEARCH_PATH` 遇到相对路径时行为未定义。
- 常见安装位置的扫描（`C:\instantclient*`、`C:\dmdbms\…` 以及 D/E 盘根目录下的目录）可能被其他本地用户预先放置恶意 DLL（CWE-427）。服务层应把找到的路径展示给用户确认，并写入设置，而不是每次都重新扫描。
- Oracle 客户端进程内只初始化一次。之后如果换了 `oracle_client_lib_dir`，新设置会被悄悄忽略，应该改为报错或给出提示。

## M1 之前

- 达梦不产生 `Message` 事件：DMSQL `PRINT` 和 `DBMS_OUTPUT` 的输出目前被丢弃。可能的实现途径是 `dpi_get_diag_field` 加 `DSQL_DIAG_PRINT_INFO`，尚未验证。
- Oracle `map_err` 还没识别连接断开的错误码（ORA-03113 / 03114 / 03135），也没识别 ORA-01005。这关系到设计文档 5.3 节的"断线检测"。
- 所有驱动都缺一个"空闲时取消"的一致性测试：没有语句在执行时调用取消，或在语句刚结束时调用取消，下一条语句都必须正常执行。达梦的取消会在服务器端延迟约 3 秒才生效，Oracle 在空闲时调用 `break_execution` 的行为也需要验证。
- 达梦 `close()` 应该先显式回滚、再登出，并把错误报告出来；目前在 Drop 时会吞掉 `dpi_logout` / `dpi_free_*` 的失败（设计文档第 8 节要求不吞错误）。
- PL/SQL 块抛错时，它之前已经输出的 DBMS_OUTPUT 行会被丢弃，需要在 M1 的结果面板里给这些行留一个展示位置。
- Oracle 的字符串参数按 NVARCHAR2 绑定，写入 `LONG` 列时，超过约 16383 个字符的数据会损坏。写 CLOB 没有这个问题。长字符串应该改为按 CLOB 绑定。
- 在 Oracle 的自动提交模式下，`SELECT … FOR UPDATE` 加的锁会一直保持，因为查询之后不会提交。
- Instant Client 在 macOS、Linux、ARM64 上的加载方式还没验证（设计文档第 12 节）。CI 中的 Oracle 任务也还没有真正运行过（仓库还没有远程地址）。
- 执行中途被丢弃的 future 不会停止对应的语句：服务层在丢弃执行流时应主动调用取消。

## 不急（可在相关模块改动时顺手处理）

- `LobPreview.total_len` 的单位在不同驱动之间不一致：Oracle CLOB 按 UTF-16 码元计，其他驱动按字节计。目前已在注释里写明，以后应统一成一种单位，或加一个单位字段。
- 达梦：
  - 执行路径缺少防 panic 的 RAII 守卫：如果在 `set_current(Some)` 和构造 `DmExec` 之间发生 panic，"当前语句"槽位不会被清除。
  - 绑定参数的缓冲区应该挪到闭包外面，保证它比 `stmt` 活得更久，与文档约定一致。
  - 连续两次取消、而第二次 `dpi_cancel` 失败时，会把第一次设置的标志清掉。
  - 对计算列，`with_scale` 可能补出很多个 0。
  - 带时区时间戳的文本格式还没验证（解析失败时会退回为 `Other`）。
  - 列名超过 256 字节时会被截断（达梦标识符最长 128 字节）。
  - `required_functions!` 清单靠手工维护。
- Oracle：
  - DBMS_OUTPUT 每读一行就是一次网络往返，且没有上限，可以改用 `GET_LINES` 批量读取。
  - 从 Oracle 函数内部产生的 DBMS_OUTPUT 输出收不到。
  - `DBMS_OUTPUT.ENABLE` 失败时，整个连接都会失败。
  - 自动提交模式下，执行 SAVEPOINT / SET TRANSACTION 之后也会提交。
  - 缺 NCLOB、BLOB、`map_err` 和 SID 连接串的单元测试。
- 阻塞适配器：
  - 工作线程 panic 时，错误信息里没有 panic 的详情。
  - `reject()` 里有重复代码。
  - tokio 同时出现在普通依赖和开发依赖里。
- 原生库查找：Windows 下前缀匹配区分大小写；配置的路径如果指向文件而不是目录，提示信息不够明确。
- CI：Oracle 镜像使用浮动标签 `23-slim`；README 里没有本地启动 Oracle 测试库的说明。
