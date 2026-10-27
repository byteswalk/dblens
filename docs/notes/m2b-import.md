# M2b 导入

依据：设计文档 `docs/superpowers/specs/2026-10-27-m2b-import-design.md`。M2 的第二份（M2a 表数据编辑已完成），M2 到此完成。

## 结构

```text
crates/dblens-dialect/src/
  split.rs     StreamSplitter：分块拆分（split_statements 一次喂入全文），语句带起始行号
  import.rs    导入的写入语句（多行 INSERT、Oracle 的 INSERT ALL、按主键更新）、DELETE 全表、建表语句
crates/dblens-service/src/
  import/file.rs     读取文件：CSV（自动判断编码和分隔符）、Excel（calamine）；open_text 也给 SQL 文件用
  import/values.rs   推断新建表的列类型；按目标列的类型转换值
  import/job.rs      导入任务：Service::preview_import / import_ddl / start_import
  sqlfile.rs         执行 SQL 文件的任务：Service::start_sql_file（扫描、确认、执行、日志）
  task.rs            TaskManager：任务的状态、取消、等待确认、报告文件、过期清理；上传的文件
crates/dblens-api/src/tasks.rs         /uploads、/imports/preview、/imports、/profiles/{id}/imports/ddl、/sql-files、/tasks/...
crates/dblens-desktop/src/shell.rs     open_dialog（系统"打开"对话框）
web/src/features/import/
  logic.ts           格式、列的自动映射、新建表的列和类型、预览单元格的校验
  FileField.tsx      选文件：桌面版用本机路径，浏览器模式上传（XHR，显示进度）
  ImportDialog.tsx   导入向导（选文件、目标与列映射、写入选项）
  SqlFileDialog.tsx  执行 SQL 文件对话框
  track.tsx          任务的进度、确认框、结果通知、下载报告、失败的语句
```

入口：标题栏"文件"菜单；对象树的连接（执行 SQL 文件）、schema（导入到新表、执行 SQL 文件）、表（导入数据、导出数据）、视图（导出数据）；数据标签工具栏的"导入"。

## 分块拆分

- 块末尾不足以判断的结构等下一块：被切开的结束符、未闭合的字符串和注释、未读到换行的行首命令（DELIMITER、`/`）、PL/SQL 块开头的关键字。能提前确定不是命令的行不必等到换行。
- 内存只与当前语句的长度有关：每块之后丢掉当前语句之前的文本（多留 4 个字节供向前查看）。
- 测试：所有已有用例的文本按随机长度切块喂入、以及在每个字符位置切成两块喂入，结果与整段拆分完全一致。
- 已知限制：跨越多块的单个字符串、注释或 `$$` 函数体，每来一块都从它的开头重新查找结尾。

## 导入

- 写入语句每条至多 500 行、30000 个参数。
- Oracle、达梦的按主键更新是每行一条 MERGE。设计文档写的是多行用 `UNION ALL` 拼来源；各行的 NULL 参数类型不一致时会报 ORA-01790，改为每行一条。
- PG 的时间间隔和不认识的类型按文本传，语句中写 `$n::text::类型`；精确小数同 M2a（`::text::numeric`）。
- 带时区的日期时间没写时区时按本机时区（与 M2a 前端一致）。
- 只映射了主键列时的按主键更新：MySQL `ON DUPLICATE KEY UPDATE 主键 = 主键`，PG `DO NOTHING`，Oracle/达梦省略 `WHEN MATCHED`。
- 读文件在阻塞线程中进行，按批经通道交给任务；任务结束时丢掉接收端，读线程随之停止。
- 整体一个事务失败或取消时，回滚后删除本次新建的表（PG 的 DDL 本可回滚，其他库的建表已自动提交，统一用 DROP）。
- 多行语句出错时不知道是哪一行：整体事务和"出错时停止"报告这条语句包含的第一行的行号。
- 出错报告是 CSV（UTF-8 带 BOM），错误信息可能有多行（Oracle 的报错带帮助链接）。
- 预览单元格的校验在前端，用表数据编辑的解析规则（`parse.ts`）；设计文档写的是预览接口附带校验结果。只检查文本单元格，Excel 的数字和日期由后端转换。

## 执行 SQL 文件

- 扫描和执行各读一遍文件。扫描按执行策略检查每条语句：被拒绝的列出第一条并结束；需要确认或生产环境时等用户确认（前端 `confirm` 后状态立即回到进行中）。
- 查询语句的结果读完丢弃；影响行数合计显示在结果中。
- 执行日志是文本（`<文件名>-log.txt`），前 100 条失败的完整语句留在任务中，可"在新查询标签中打开"。
- 结束后清掉这个连接的元数据缓存，前端刷新对象树。

## 任务与上传

- 导入和 SQL 文件共用 TaskManager（同时最多 4 个）。设计文档写的是由 ExportManager 推广而来、导出也改用它；导出已经过实测，为避免改动它，导出仍用原来的 ExportManager。
- 上传：请求体直接写到数据目录的 `imports/`，大小上限默认 2048 MB，设置项 `ui.import.upload_limit_mb`（接口只允许写 `ui.` 开头的设置）。设计文档写的设置项名为 `import.upload_limit_mb`。
- 上传后 1 小时没有使用的文件、结束 1 小时的任务和报告自动删除；服务启动时清空 `imports/` 和 `tasks/`。
- 本机路径只有桌面程序的本机服务接受（与导出相同）。
- 预览接口不需要连接：`POST /imports/preview`。设计文档写的是 `/profiles/{id}/imports/preview`。

## 导出数据

- 对象树的"导出数据"复用 M1c3 的导出对话框，数据来源为整张表（不过滤、不排序，与打开数据标签时相同）。设计文档写的是有主键时按主键排序。

## 依赖

- 新增 `calamine` 0.37.0（MIT，特性 `chrono`）、`encoding_rs` 0.8.42（MIT / Apache-2.0）、`encoding_rs_io` 0.1.8（MIT / Apache-2.0，转码的读取器）。设计文档只列了前两个。
- 2026-10-27 写计划时复查（`node scripts/check-deps.mjs`）：共 77 个直接依赖，0 个可升级；未满 7 天的有 @tanstack/react-query 5.105.0、antd 6.6.8、i18next 26.5.11、react-i18next 17.1.9、react-resizable-panels 4.15.0、simple-icons 16.36.0、minijinja 3.0.4、rust_xlsxwriter 0.101.0、uuid 1.29.0。

## 验证

- 后端：fmt、clippy 通过；全部测试在 `DBLENS_LANG=en` 下通过。
  - 实库测试（5 个库）：导入到已有表（追加 2500 行含中文、NULL、精确小数和日期；按主键更新；清空后导入；整体事务中主键重复全部回滚；分批跳过两行并核对报告）；导入到新表（建表、写入；整体事务失败时删除新表）；SQL 文件（MySQL 的 DELIMITER 存储过程、PG 的 `$$` 函数、Oracle 和达梦的 PL/SQL 块加 `/`；出错时继续和停止）。
  - 达梦的测试与其他测试同时连库时偶发连接失败，单独运行通过（与 M1c1、M1c3 记录的相同）。
- 前端：tsc、Vitest、打包通过。
- 浏览器实测（PostgreSQL，浏览器模式）：
  - schema 右键"导入到新表"：上传 CSV，预览识别 UTF-8 和逗号；推断类型、勾选主键后建表语句带 `PRIMARY KEY`；导入 300 行，"打开数据"打开数据标签；
  - 数据标签"导入"：自动按列名映射，预览中不能转换的值标出；按主键更新加出错时跳过，写入 20 行、跳过 1 行，出错报告的行号和原因正确；
  - 连接右键"执行 SQL 文件"：出错时继续，失败的语句可在新查询标签中打开；生产环境的连接列出需要确认的 DELETE、DROP，确认后执行；
  - 表右键"导出数据"打开导出对话框。
  - 执行计划时在英文界面走了一遍导入到新表和标题栏"文件 → 执行 SQL 文件"，发现并修正：自动判断的编码和分隔符在英文界面用了中文括号（改为按语言的词条 `import.file.autoWith`）；英文词条补上单复数（`_one` / `_other`），几个数字并列的进度和结果改为"名称: 数字"的写法。
  - 内置浏览器不能操作系统的文件选择框：实测时用脚本给隐藏的文件输入框放入文件。
- 未验证：桌面版的系统"打开"对话框和报告的"另存为"需要在桌面程序中手动操作一次。
