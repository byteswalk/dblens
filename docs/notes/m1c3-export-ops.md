# M1c3 导出与运维查询

依据：设计文档 `docs/superpowers/specs/2026-10-26-m1c3-export-ops-design.md`。M1c 的第三份，也是 M1 的最后一份（M1c1 历史与收藏、M1c2 对象操作已完成）。

## 结构

```text
crates/dblens-driver/src/{traits.rs, blocking.rs, conformance.rs}
                  Connection::set_lob_limit：大字段的读取上限；阻塞适配器转发；一致性测试 lob_limit_reads_full_value
crates/dblens-driver-{mysql,postgres,oracle,dameng}/src/   各驱动按连接上的上限读取大字段（原来写死 64 KB）
crates/dblens-dialect/
  sql/ops/<方言>.sql   运维查询模板：-- [key] 分段，段首注释写中英文名称和分类；mariadb.sql 只覆盖 locks
  src/ops.rs          解析模板（保持顺序）、Dialect::ops_queries、兼容库回退到所属家族
crates/dblens-service/src/export/
  write.rs            四种格式的写入（RowWriter）：CSV（csv 库）、xlsx（rust_xlsxwriter 低内存模式）、INSERT（方言字面量）、JSON
  job.rs              导出任务：Service::start_export、ExportManager（状态、取消、下载、过期清理）
crates/dblens-service/src/service.rs   Service::ops（运维查询列表）、exports、history
crates/dblens-api/src/exports.rs       /exports、/exports/{id}、/exports/{id}/file、/profiles/{id}/ops
crates/dblens-desktop/src/shell.rs     save_dialog（另存为）、reveal_in_folder（在文件夹中显示）
web/src/
  features/export/   导出对话框、默认选项和文件名、进度通知
  features/ops/      运维查询侧栏
  features/editor/   打开后执行的标记（markRun / takeRun）、结果面板和数据标签的导出入口
```

## 导出

- 任务另开一个连接（`registry.connect`），不经过会话管理：
  - 连接上设置大字段读取上限 256 MB，执行、每批读 1000 行、写文件，结束后关闭；
  - 只导出第一个结果集；
  - 照常写一条执行历史（行数为写出的行数）。

  设计文档写的是"经 SessionManager"。直接用连接更简单：会话的事件接口是给界面用的，导出不需要确认、事务和 schema 跟踪。
- 只接受单条查询语句：方言拆分后只有一条，且语句分析为查询（DQL）。数据标签的导出由后端用 `data_sql` 生成 SELECT。
- 取消：
  - 先置取消标志，再取连接的取消句柄；任务存好句柄后先检查标志。这样无论取消在连接前还是执行中到达，都能结束。实现时测试发现过这个竞态。
  - 读取循环每批检查一次标志：语句执行完、正在读取时，数据库层面的取消可能不起作用。
- 失败、取消时删除已写的文件。用户在"另存为"中选了已有文件时，原文件会被覆盖，取消后不会恢复。
- 浏览器模式：
  - 临时文件放在数据目录的 `exports/` 下，服务启动时清空。设计文档写的是退出时清空，启动时清空效果相同，也能清掉异常退出留下的文件。
  - 下载用流式响应，传完或连接中断后删除文件。
  - 前端轮询到完成后，用带令牌的链接触发下载。
- 错误：任务状态中保存的是错误对象，序列化时按请求的语言渲染。
- 格式细节（单元测试覆盖）：
  - CSV：RFC 4180 转义；可选 BOM；NULL 写成空或 `NULL`。
  - xlsx：表头加粗；数字、日期、时间按类型写入；精确小数写成文本（避免经过浮点）；单元格超过 32767 个字符时截断，计入警告；1048575 行后续到下一个工作表。
  - INSERT：值用方言的字面量（PG 的二进制为 `decode('…','hex')`）；可以多行合并成一条。
  - JSON：重复的列名加后缀 `_2`、`_3`；数字写成 JSON 数字，精确小数写成字符串。

## 运维查询

- 每种数据库 6 项：会话列表、正在执行的语句、锁等待、存储（Oracle、达梦为表空间使用，MySQL、PG 为各库大小）、占用最大的表、慢 SQL。
- 会话、锁的结果带 `kill_sql` 列：MySQL 系 `KILL id`，PG 系 `pg_terminate_backend`，Oracle `ALTER SYSTEM KILL SESSION 'sid,serial#' IMMEDIATE`，达梦 `SP_CLOSE_SESSION`。
- 列表接口不连接数据库：按连接上次识别出的变体选模板，没连接过时按驱动。设计文档没有写明，这样打开侧栏不会触发连接（也不会弹密码库解锁）。
- 侧栏：点击用 `openSql(..., { run: true })` 新开标签；查询标签第一次显示时取走"打开后执行"的标记并执行全文。标记不随标签保存，重新打开页面不会再执行。
- 测试库上的结果：
  - Oracle、达梦的测试账号能读 V$ 和 DBA_ 视图，6 项都能执行；
  - PostgreSQL 没有安装 pg_stat_statements 扩展，"慢 SQL"报"关系 pg_stat_statements 不存在"（按设计显示数据库的报错）；
  - 其余全部通过。

## 依赖

- 新增 `csv` 1.4.0（MIT / Unlicense）。
- 新增 `rust_xlsxwriter` 0.100.0（MIT / Apache-2.0，特性 `constant_memory`、`chrono`）。写计划时最新的 0.101.0 发布未满 7 天。
- `dblens-api` 的 tokio 多开 `fs`、`io-util` 特性（下载时流式读文件）。

## 验证

- 后端：fmt、clippy 通过；全部测试在 `DBLENS_LANG=en` 下对 5 个实库通过。
  - 驱动一致性测试：放宽上限后读出 100000 字节的大字段，改回默认后又是预览（4 个驱动，MySQL 和 MariaDB 各一次）。
  - 导出实库测试（5 个库）：一行含中文和 100000 个字符的大字段，CSV、JSON、INSERT 都写出完整内容（字符数与数据库报告的一致）；xlsx 截断并报一个警告；另导出 2500 行（MySQL 系 990 行）核对行数。
  - 运维查询实库测试（5 个库）：逐个执行全部 6 项，只允许 PG 的慢 SQL 失败。
  - 有一次全量运行中运维查询的实库测试失败一次，之后连续 12 次通过，判断与 M1c1 记录的情况相同：多个测试同时连达梦时偶发连接失败。
- 前端：tsc、Vitest、打包通过。
- 浏览器实测（浏览器模式，5 个库建测试数据）：
  - 中文界面：运维查询侧栏按分类列出；点"会话列表"新开标签并执行，结果带 `kill_sql` 列。结果面板导出 CSV：完成通知显示行数，下载链接返回正确的内容。数据标签加过滤 `parent_id = 2` 后导出 INSERT：默认目标表为 `public.c2_child`，文件名为对象名加时间，400 条语句。
  - 英文界面：侧栏和导出对话框的文字；侧栏跟随当前标签的连接，切到 Oracle（生产环境）执行"表空间使用"。
  - 内置浏览器不能真正下载文件：实测时拦截了下载链接的点击，用请求取回文件内容核对。
- 未验证：桌面版的"另存为"对话框和"打开所在文件夹"需要在桌面程序中手动操作一次（系统对话框不能由自动化点击）。
