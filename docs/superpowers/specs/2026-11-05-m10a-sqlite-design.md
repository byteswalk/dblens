# M10a SQLite：连接、浏览、查询 设计

日期：2026-11-05（与用户确认：只在桌面版、分两步、不支持加密库，其余按推荐方案）

## 1. 目标

支持 SQLite 文件库：连接、浏览、执行 SQL（含触发器脚本）、编辑结果、导出、运维页、文档导出（只读元数据，不用拦）。

表设计器、导入、结构比对、保存为模型、迁移、规范检查留给 M10b：后端报 `sqlite.later`，前端入口置灰并提示，迁移和比对的来源、目标列表不出现 SQLite。

## 2. 范围

- 只在本机文件可用时（`ApiConfig::local_files`：桌面版，或只监听本机地址的服务器版）开放。其他情况连接报“只有桌面版支持 SQLite”，产品列表不出现 SQLite。审计版不开放。
- 用程序自带的 SQLite（rusqlite 0.41.1 `bundled`，静态编入，打开了 dbstat），不依赖系统库。
- 不做：加密库（SQLCipher；打开时提示“不是数据库文件或已加密”）、加载扩展、journal/WAL 设置界面、rowid 定位编辑（没有主键的表和其他库一样不能编辑）。

## 3. 连接

- 连接配置：`database` 存文件的绝对路径，`host` 为空、`port` 为 0、用户名为空、不存密码。
- 选项：`read_only=1` 只读打开；`create=1` 文件不存在时新建（默认不建，路径写错时报“文件不存在”）。
- 打开后设 `busy_timeout` 5 秒（元数据连接和会话连接同时访问同一文件）、`foreign_keys=ON`。
- 元数据连接池、缓存的范围按连接配置 id，和其他库一样。

## 4. 驱动

新 crate `dblens-driver-sqlite`，`DriverId::Sqlite`（`sqlite`），经 `blocking::into_async` 包装，每连接一个线程。

- 能力：`catalogs: false`、`schemas: true`、`transactional_ddl: true`、`cancel: true`、`explain: true`、`multiple_result_sets: false`。
- 取数：语句准备后逐行 `step`，按批交出，不把结果全读进内存。结果集持有语句（Box 固定地址，行游标先于语句释放）。
- 取消：`sqlite3_interrupt`（rusqlite `InterruptHandle`），返回 `SQLITE_INTERRUPT` 转为取消错误。
- 事务：自动提交模式下语句各自生效，用户自己写 `BEGIN`/`COMMIT` 也照常。手动模式下执行语句前若不在事务中先 `BEGIN`；`commit`、`rollback` 在事务中才执行；从手动切回自动时提交未完成的事务。
- 服务器信息：`product = "SQLite"`，`version = sqlite_version()`。
- 错误：`SQLITE_BUSY`/`LOCKED` 原样报出；`SQLITE_NOTADB` 提示“不是数据库文件，或已加密”；`SQLITE_CANTOPEN` 提示文件不存在或无权限。

### 4.1 值

按每个值的存储类别取值，不按声明类型猜：INTEGER → 整数，REAL → 浮点，TEXT → 字符串，BLOB → 二进制（超过预览上限截断为 `Value::Lob`），NULL → 空。列元信息的类型名用声明类型（表达式列为空）。

参数绑定：整数、浮点、字符串、二进制照写；布尔写 1/0；小数、日期、时间、时间戳写文本（`YYYY-MM-DD HH:MM:SS[.ffffff]`，带时区的写 `±HH:MM`）；JSON 写文本；UUID 写文本。

## 5. 方言

- 新家族 `Family::Sqlite`、变体 `Variant::Sqlite`，标识符双引号，占位符 `?`，分页 `LIMIT … OFFSET`，布尔字面量 1/0。
- 逻辑类型按 SQLite 亲和性规则：含 INT → 整数；含 CHAR/CLOB/TEXT → 文本（保留长度）；含 BLOB 或空 → 二进制/其他；含 REAL/FLOA/DOUB → 浮点；`DECIMAL(p,s)`/`NUMERIC(p,s)` → 小数；`DATE`、`DATETIME`、`TIMESTAMP`、`TIME`、`BOOLEAN`、`JSON`、`UUID` 按名字映射。
- 语句拆分：`CREATE [TEMP] TRIGGER` 到与之配对的 `END` 才结束（`CASE … END` 计入配对）。
- 运维页：存储概况（文件大小、页大小、页数、空闲页、journal 模式、编码）和最大的表（`dbstat`，bundled 已启用）。没有会话、锁、慢 SQL。

### 5.1 元数据

纯 SQL：`"schema".sqlite_schema` 加表值函数 `pragma_table_xinfo`、`pragma_index_list`、`pragma_index_xinfo`、`pragma_foreign_key_list`、`pragma_database_list`。

- schema：`pragma_database_list` 中除 `temp` 以外的库（`main` 和 ATTACH 进来的）。
- 对象：表、视图（含其列）、触发器、索引。没有序列、过程、同义词、物化视图、注释、权限。`sqlite_*` 内部表不列出。
- 主键：`table_xinfo.pk`；外键：`foreign_key_list`（名字为空）；唯一约束：`index_list.origin = 'u'`；普通索引：`origin = 'c'`，表达式索引列取索引定义。
- 自增：`INTEGER PRIMARY KEY`（rowid 别名，非 WITHOUT ROWID）算自增。
- 生成列：`table_xinfo.hidden` 为 2（VIRTUAL）、3（STORED）。
- 检查约束、生成列表达式、约束名、表达式索引列、触发器的时机和事件系统目录里没有：从 `sqlite_schema.sql` 的原文读（`meta/sqlite.rs`，只扫描引号、注释和括号层级，表达式按原文切出）。原计划用 sqlparser，但它的 AST 输出的是规范化后的文本，原文切片能保留用户的写法；读不懂只缺这几项，不报错。
- DDL：表、视图、索引、触发器直接用 `sqlite_schema.sql` 的原文。

## 6. 服务和前端

- 注册表在 `local_files` 打开时才接受 `DriverId::Sqlite`；驱动状态里 SQLite 在不可用时给出提示。
- 连接表单：选 SQLite 后只显示“文件”（输入框 + “选择…”调用原生对话框）、“只读打开”“文件不存在时新建”，以及环境、颜色。测试连接、对象树、编辑器方言、图标照常。
- M10b 的功能入口置灰并提示（同 GBase 8s 在 M8a 时的做法）。

## 7. 测试

- 驱动一致性测试（conformance）用临时文件，无需外部服务。
- 方言单元测试：类型亲和性、拆分触发器、建表语句解析。
- 元数据实测：夹具建表（主键、复合主键、外键、唯一、检查、生成列、表达式索引、降序索引、视图、触发器、WITHOUT ROWID、ATTACH）后核对。
- 服务层：连接、会话执行、取消、编辑写回、导出、运维页；`local_files` 关闭时拒绝。
- 桌面窗口：真实鼠标操作新建连接、浏览、查询、编辑并截图。
