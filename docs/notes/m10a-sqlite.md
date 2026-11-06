# M10a SQLite：连接、浏览、查询

设计见 `docs/superpowers/specs/2026-11-05-m10a-sqlite-design.md`。覆盖计划第八项的第一步。

## 结构

- 新 crate `dblens-driver-sqlite`：rusqlite 0.41.1（`bundled`，静态编入 SQLite，自带 dbstat），经阻塞适配器每连接一个线程。没有新增依赖（rusqlite 原来就用于本地存储）。
- 方言：`Family::Sqlite`、`Variant::Sqlite`，元数据模板 `sql/sqlite.sql`，运维模板 `sql/ops/sqlite.sql`，建对象原文的读取在 `meta/sqlite.rs`。
- 服务：`DriverRegistry::allow_local_files`，接口层按 `ApiConfig::local_files` 设置（桌面版、只监听本机的服务器版为真）；为假时 SQLite 驱动不可用，产品列表不出现。审计版的引擎不设置，所以不开放。
- M10b 的功能在服务层报 `sqlite.later`（`later.rs`），前端菜单置灰，比对和迁移的连接列表不出现 SQLite。

## 行为

- 连接配置：`database` 是文件路径，主机、端口、账号为空。选项 `read_only=1`、`create=1`；不新建时文件必须存在。
- 打开后：`busy_timeout` 5 秒，`foreign_keys=ON`，先读一次 `sqlite_schema`，不是数据库或已加密的文件在连接时就报错。
- 值按实际存储类别取；日期、小数绑定为文本。DDL 的影响行数报 0（不沿用上一条 DML 的 `changes()`）。
- 取消用 `sqlite3_interrupt`。手动提交模式下执行语句前自动 `BEGIN`。
- 当前 schema 固定为 `main`，不能切换。执行计划用 `EXPLAIN QUERY PLAN`。
- 语句拆分：`CREATE [TEMP] TRIGGER … BEGIN … END` 是一条（`CASE … END` 计入配对）。
- 生产环境的危险语句确认用 sqlparser 的 SQLite 方言判断有没有 WHERE。

## 测试

- 驱动：一致性测试全套（临时文件，不需要外部服务）加 DDL 影响行数、日期绑定、出错位置、只读、非数据库文件、中途丢弃结果流。
- 方言：类型亲和性、触发器拆分、建表语句解析的单元测试；元数据实测（主键、复合主键、外键、唯一、检查、生成列、表达式索引、降序、视图、触发器、WITHOUT ROWID、ATTACH）；SQL 快照。
- 服务：完整流程、工作台、取消、运维页、导出四种格式、数据编辑（含冲突）、SQL 文件（触发器）、M10b 拦截、`local_files` 关闭时拒绝。
- 接口：`/api/drivers` 的可用状态、测试连接按 `local_files` 放行或拒绝。
- 前端：连接表单的转换和校验、图标。
- 界面（2026-11-05，只监听本机的 dblens-server 加浏览器，真实鼠标键盘）：新建连接只显示文件、打开方式、环境，测试连接识别为 SQLite 3.54.1；对象树只有表、视图，`sqlite_sequence` 不列出；查看数据（中文、实数、日期文本、BLOB、生成列）；改一个单元格提交，生成 `UPDATE main.emp SET salary = 9900 WHERE id = 2;`，文件里确认改了；编辑器执行带触发器的脚本，触发器体没被拆开；设计表、导入置灰并说明；连接和表的菜单里没有权限项。

## 已知局限

- 编辑器的语法检查按 PG 的宽松档，SQLite 的触发器等写法会标黄（不是报错）。
- 标题栏的“导入数据”、模型面板的“从库生成模型”先选连接，选了 SQLite 时由后端报“暂不支持”，没有在前端置灰。
- 桌面版的“选择…”文件对话框没有在桌面窗口里实测（浏览器模式下不显示这个按钮），路径输入已测。
- 产品说明写的是“只有桌面版能打开”；只监听本机的服务器版其实也能用。
