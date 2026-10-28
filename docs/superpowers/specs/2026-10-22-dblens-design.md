# DBLens（数洞）设计文档

- 日期：2026-10-22
- 状态：待审阅
- 范围：整体架构，M0（地基）与 M1（查询工作台）的详细设计，M2–M5 的边界概要

## 1. 背景与目标

在三个老工具的基础上，做一个独立的跨平台数据库管理工具。三个老工具是 STP4E33、dbsecurity 和 dbdesigner，其中损坏源码的恢复情况见仓库外的 `../_recovered/README.md`。

老工具的共同局限：
- 只支持 Oracle；
- 绑定 Windows；
- 凭据保护薄弱；
- 存在 SQL 注入风险；
- 全局状态多，同一时间只能连一个库。

新工具只继承老工具的功能思路和少量算法，不移植代码。

**定位**
- 以个人开发者客户端为主：连接管理、对象浏览、SQL 编辑执行、数据编辑。
- 同时提供建模能力：表设计、结构比对、ER 图、文档。
- 架构上为以后的团队管控（服务端、多用户、SQL 审核与审计）预留扩展点。

**产品名**：DBLens，中文名"数洞"。crate 和包名前缀为 `dblens`。

## 2. 硬性约束

| 项目 | 决定 |
|---|---|
| 技术栈 | 后端 Rust；前端 React + TypeScript（Ant Design、Monaco、Glide Data Grid、React Flow） |
| 产品形态 | 以 Tauri 2 桌面应用为主；同一后端可作为独立 HTTP 服务（`dblens-server`）启动，供浏览器访问 |
| 首期数据库 | MySQL 系（MySQL、MariaDB、OceanBase-MySQL 模式、TiDB、PolarDB-MySQL）；PostgreSQL 系（PostgreSQL、人大金仓、openGauss/GaussDB、瀚高）；Oracle；达梦 DM |
| 平台 | Windows x64、macOS（Intel 和 Apple Silicon）、Linux x64、信创（统信 UOS、银河麒麟；ARM64；LoongArch 尽力支持） |
| 驱动策略 | 方案 C：统一驱动接口，原生实现优先，JDBC 桥作为后续补位 |

## 3. 里程碑

| 里程碑 | 内容 | 依赖 |
|---|---|---|
| M0 地基 | Tauri 外壳和浏览器模式、四类驱动、统一结构模型、方言接口、连接管理 | — |
| M1 查询工作台 | 对象树、SQL 编辑器、多语句执行与取消、事务、结果流式加载、导出结果、历史与收藏 | M0 |
| M2 数据 | 表数据编辑、CSV/Excel/SQL 导入导出 | M1 |
| M3 结构设计 | 表设计器，按方言生成 DDL | M0 |
| M4 模型 | 模型文件、反向生成模型、结构比对与同步、ER 图、数据字典和设计文档导出 | M3 |
| M5 迁移 | 库到库的结构和数据迁移 | M2、M3 |
| 后续 | 团队管控、JDBC 桥、更多数据库（SQLite、SQL Server 等） | 预留接口 |

每个里程碑各自走一遍"设计 → 实现计划 → 实现"。本文档详细描述 M0 和 M1，M2–M5 只定边界。

## 4. 整体架构

```
┌──────────── 前端 web/（React + TS，两种模式共用） ────────────┐
│ 对象树 │ SQL 编辑器(Monaco) │ 结果表格(Glide) │ 设计器/ER(React Flow) │
└──────────────┬──────────────────────────────────────┘
               │ HTTP (REST) + WebSocket（流式结果、进度、取消）
┌──────────────┴──────────────────────────────────────┐
│ api（axum）：路由、令牌鉴权、Origin 校验、请求校验              │
│ service：会话、查询执行、元数据缓存、本地存储、Policy 钩子       │
│ dialect（无 IO）：元数据查询模板、DDL/DML 生成、类型映射、语句拆分 │
│ driver：统一的 Driver/Connection 接口                         │
│   mysql │ postgres │ oracle(运行时加载 OCI) │ dameng(运行时加载 DPI) │
│   （后续）jdbc-bridge 外部进程                                  │
└─────────────────────────────────────────────────────┘
```

### 4.1 通信方式

- 前端在两种模式下都只通过 HTTP + WebSocket 调用业务，不用 Tauri IPC 调业务。
- 桌面模式：Tauri 进程内启动 axum，只绑定 `127.0.0.1` 的随机端口。每次启动生成一个随机令牌，经 Tauri 注入前端。服务端校验令牌和 Origin。
- 浏览器模式：`dblens-server` 监听可配置的地址。首期仅供单用户使用，启动时打印一次性访问令牌；多用户鉴权留给后续的团队版。
- Tauri 只负责原生能力：窗口、菜单、系统文件对话框、系统钥匙串、自动更新。

### 4.2 Cargo 工作区

| crate | 职责 |
|---|---|
| `dblens-core` | 公共类型：`Value`、列类型（原生类型 + 逻辑类型）、结构模型、错误类型 |
| `dblens-driver` | `Driver`、`Connection`、`ExecStream`、`CancelHandle` 等接口；一致性测试套件 |
| `dblens-driver-mysql` / `-postgres` / `-oracle` / `-dameng` | 四个驱动实现，各用 feature 开关控制 |
| `dblens-dialect` | 各方言的元数据查询模板（minijinja，按显式 key 取用）、DDL/DML 生成、类型映射、语句拆分 |
| `dblens-service` | 连接配置、会话管理、查询执行、元数据缓存、本地存储、`Policy` 接口 |
| `dblens-api` | axum 路由与 WebSocket 协议 |
| `dblens-desktop` | Tauri 应用入口 |
| `dblens-server` | 浏览器模式的服务入口 |
| `web/` | 前端 |

### 4.3 本地数据

- 位置：用户数据目录下的 SQLite 文件（如 Windows 的 `%APPDATA%/DBLens/dblens.db`）。
- 内容：连接配置（不含密码）、执行历史、收藏 SQL、界面状态。
- 密码：存进系统钥匙串（Windows 凭据管理器、macOS Keychain、Linux Secret Service）。如果系统没有钥匙串，就用主密码派生密钥（Argon2）加密后存入本地库。
- 日志：按天滚动，存在用户数据目录下的 `logs/`。

## 5. 驱动层（M0）

### 5.1 接口

```rust
trait Driver: Send + Sync {
    fn id(&self) -> DriverId;                 // mysql / postgres / oracle / dameng
    fn capabilities(&self) -> Capabilities;   // 是否有 catalog/schema、DDL 是否支持事务、能否取消、能否 explain 等
    async fn connect(&self, p: &ConnectParams) -> Result<Box<dyn Connection>>;
}

trait Connection: Send {
    async fn execute(&mut self, sql: &str, params: &[Value]) -> Result<ExecStream>;
    fn cancel_handle(&self) -> CancelHandle;  // 可以从其他线程或任务调用
    async fn set_autocommit(&mut self, on: bool) -> Result<()>;
    async fn commit(&mut self) -> Result<()>;
    async fn rollback(&mut self) -> Result<()>;
    async fn ping(&mut self) -> Result<()>;
    async fn server_info(&mut self) -> Result<ServerInfo>; // 产品名、版本，用于识别兼容库
    async fn close(self: Box<Self>) -> Result<()>;
}

enum ExecEvent {
    ResultStart { columns: Vec<ColumnMeta> }, // 列名、原生类型名、精度、是否可空、来源表和列（如果能取到）
    Rows(Vec<Row>),                           // 按批产生，消费方拉取下一批时才继续读游标
    ResultEnd,
    Affected(u64),
    Message(String),                          // PG notice、DBMS_OUTPUT
}
```

### 5.2 各驱动实现

| 驱动 | 底层 | 说明 |
|---|---|---|
| mysql | `mysql_async`（纯 Rust） | 取消方式：另开一个连接执行 `KILL QUERY <id>` |
| postgres | `tokio-postgres`（纯 Rust） | 用 cancel token 取消；notice 映射为 `Message` |
| oracle | `oracle` crate（ODPI-C），运行时加载 Instant Client | 同步接口，跑在专用工作线程上；用 break execution 取消；DBMS_OUTPUT 在执行后读取 |
| dameng | 自行编写 DPI 的 FFI 绑定，运行时用 `libloading` 加载 `dmdpi` | 同步接口，跑在专用工作线程上。如果 DPI 有走不通的地方，就改走 ODBC（`odbc-api`），对外接口不变 |

- **同步驱动的包装**：每个连接配一个专用线程，通过命令通道收发消息，对外仍是异步的 `Connection`。JDBC 桥以后用同样的模式（命令发给外部进程）。
- **原生库查找顺序**：设置里指定的路径 → 环境变量（`ORACLE_HOME`、`DM_HOME`、`LD_LIBRARY_PATH` 等） → 平台上的常见安装位置。
  - 找不到时，只有该驱动显示为"不可用"，并给出安装指引，应用照常启动。
  - Oracle 和达梦的客户端库不随安装包分发。
- **值类型 `Value`**：Null、Bool、Int(i64)、UInt(u64)、Float(f64)、Decimal（字符串保存，不丢精度）、String、Bytes、Date、Time、DateTime、DateTimeTz、Interval、Json、Uuid、Lob 预览（只取前 N KB，带一个"整体获取"的句柄）、`Other { type_name, text }`。

### 5.3 会话模型

- 每个 SQL 编辑器标签页独占一个连接（称为 Session），事务、会话变量、临时表都只作用于本页。
- 每个连接配置再配一个小连接池（默认 2 个连接），专门用于元数据浏览，不会被正在跑的长查询堵住。
- 默认自动提交，可以切换为手动提交。有未提交的事务时，标签页上明确提示；关闭标签页时询问是提交还是回滚。
- 连接断开时检测出来，下一次执行前询问是否重连，不静默重连。

## 6. 结构模型与方言层（M0）

### 6.1 结构模型（`dblens-core`）

```
Connection
 └ Catalog（MySQL/PG 的 database；Oracle 和达梦没有这一层）
    └ Schema（PG schema / Oracle、达梦的用户 / MySQL 的 database 同时充当 schema）
       ├ Table：Column[]、PrimaryKey、Unique[]、ForeignKey[]、Check[]、Index[]、Trigger[]、注释、分区信息（首期只读）
       ├ View / MaterializedView：定义 SQL、列
       ├ Sequence
       ├ Routine：存储过程、函数、包（首期只读查看源码）
       └ Synonym（Oracle、达梦）
```

- 列类型同时保存两份：
  - `native`：原样的类型文本，如 `VARCHAR2(100 CHAR)`，用于展示和同库 DDL；
  - `logical`：枚举形式的逻辑类型，如 `VarChar{len}`、`Decimal{p,s}`、`Timestamp{tz,precision}`，用于跨库映射和设计器。
- 所有模型都实现 `serde` 序列化：API 层传 JSON，M4 的模型文件存成 YAML。

### 6.2 方言接口（`dblens-dialect`）

1. **读取元数据**
   - 每个方言一个查询模板文件，用 minijinja 渲染，按显式的 key 取模板。
   - 查询必须批量：一个 schema 的列、约束、索引各用一条 SQL 取回。
   - 对象树懒加载：先列名字，展开时取详情。
   - 支持在服务端按名字和注释过滤。
   - 所有参数一律用绑定变量，不拼接字符串。
2. **生成 SQL**
   - 先把操作表示成变更列表：`CreateTable`、`AddColumn`、`AlterColumn`、`DropColumn`、`AddConstraint`、`AddIndex`、`SetComment` 等。
   - 方言把变更列表生成为 DDL。
   - 另外提供标识符引用、字面量格式化、分页包装、SELECT/INSERT/UPDATE/DELETE 模板。
3. **类型映射**：原生类型与逻辑类型之间的双向转换。
4. **语句拆分**
   - 要处理各库的语法：Oracle/达梦的 PL/SQL 块和 `/` 结束符，MySQL 的 `DELIMITER`，PG 的 `$$` 引号，以及字符串和注释。
   - 输出每条语句及其在原文中的起止位置。

### 6.3 兼容库

- 连接后用 `server_info()` 识别具体产品和版本，选出方言变体：
  - 金仓、openGauss、瀚高 → PG 方言的变体；
  - OceanBase(MySQL)、TiDB、PolarDB → MySQL 方言的变体。
- 变体只覆盖有差异的个别查询模板或生成规则，不另写驱动。

### 6.4 元数据缓存

- 由服务层按连接缓存。
- 手动刷新时失效。
- 在某个会话中执行了 DDL 后，所涉及的 schema 自动失效。

## 7. 查询工作台（M1）

### 7.1 界面

- 布局：
  - 左侧：连接列表和对象树；
  - 中间：多标签页（SQL 编辑器、表数据、对象详情）；
  - 下方：结果面板（每个结果集一个标签，另有消息和历史）。
- 连接可以标记环境：开发、测试、生产，各有颜色。生产连接的标签页有醒目的红色标识。
- 设计稿：前端开始实现之前，先出界面设计稿，经确认后再写代码。

### 7.2 桌面窗口行为

- **默认大小**：首次启动 1440×900，屏幕居中。屏幕小于这个尺寸时，按工作区的 90% 缩放。最小尺寸 1024×640。
- **主题**：亮色和暗色跟随系统，切换系统主题时实时生效。设置里可以固定为亮色或暗色。Monaco 和结果表格同步切换。
- **置顶**：菜单或标题栏里有"窗口置顶"开关，配快捷键，状态会保存。
- **记住窗口状态**：退出时保存位置、大小和是否最大化。菜单里有"恢复默认窗口位置和大小"（回到居中和默认尺寸）。如果保存的位置落在已经不存在的显示器上，启动时移回主屏幕。

### 7.3 对象树

- 懒加载，顶部有过滤框，可按名字或注释过滤。
- 右键菜单：
  - 查看数据；
  - 查看 DDL；
  - 生成 SELECT/INSERT/UPDATE/DELETE（SELECT 的每列后附注释）；
  - 复制名称；
  - 刷新。
- 选中多张表时，可以"生成关联查询"：优先按真实外键推断 JOIN，没有外键时再按同名主键列推断。

### 7.4 SQL 编辑器

- 基于 Monaco，按方言切换语法高亮。PL/SQL 的高亮规则需要自己编写（Monarch）。
- 自动补全：
  - 关键字；
  - 从元数据缓存取的 schema、表、列；
  - 能识别 `FROM t a` 这类别名，`a.` 之后提示 t 的列。
- 格式化：用 `sql-formatter`，按方言选择 plsql、mysql 或 postgresql。
- 执行：
  - 执行当前语句（Ctrl+Enter）、执行选中部分、执行全部（F5）；
  - 多条语句按顺序执行，"出错停止"或"出错继续"可选；
  - 有停止按钮，显示耗时和影响行数；
  - SQL 错误按原生错误码和位置，在编辑器中标红。
- 执行计划：M1 先以表格或文本形式展示。

### 7.5 结果集

- 每个结果集首批取 500 行。向下滚动时按批继续拉取，游标在后端保持打开。
- 前端只保留已经加载的行。单个结果集的行数有上限（可配置，默认 10 万行），超出后提示改用导出。
- 导出（CSV、xlsx、INSERT 语句、JSON）由后端重新执行查询，边取边写文件。
  - 桌面模式弹出系统保存对话框；浏览器模式通过下载链接获取文件。
  - xlsx 用 `rust_xlsxwriter`，超过单个 sheet 的行数上限时自动分 sheet。
- WebSocket 消息：
  - 客户端发 `exec`、`fetch_more`、`cancel`；
  - 服务端回 `result_start`、`rows`、`result_end`、`affected`、`message`、`error`、`done`。

### 7.6 历史、收藏与内置查询

- 每次执行都写入本地历史：时间、连接、SQL、耗时、行数、是否成功。支持搜索。
- 收藏 SQL，按文件夹管理。
- 按方言内置一组常用运维查询：会话、锁及对应的 kill 语句、表空间或存储使用情况、慢 SQL。

### 7.7 Policy 钩子（为管控预留）

```rust
trait Policy: Send + Sync {
    async fn before_execute(&self, ctx: &ExecContext, stmt: &StatementInfo) -> Decision; // Allow / Confirm(reason) / Deny(reason)
    async fn after_execute(&self, ctx: &ExecContext, stmt: &StatementInfo, outcome: &Outcome);
}
```

- 所有执行路径都必须经过 `Policy`，包括 SQL 编辑器、数据编辑、DDL 执行、迁移。
- `StatementInfo` 包含语句类型（DQL/DML/DDL/DCL/PLSQL 块）、是否带 WHERE、涉及的对象。语句先用 `sqlparser-rs` 解析，解析失败时退回看首个关键字。
- 个人版自带 `DefaultPolicy`：仅在环境为"生产"的连接上，对以下语句返回 `Confirm`：
  - 不带 WHERE 的 UPDATE/DELETE；
  - DROP；
  - TRUNCATE。
- 以后的团队版通过同一接口实现：对象授权、危险级别、时间窗、执行前备份、审计。这些思路来自 dbsecurity 的 `SqlAnalysisProvider.permissionCheck`。

## 8. 错误处理

- 统一的错误枚举：`Connect`、`Auth`、`Sql { code, message, position }`、`Timeout`、`Cancelled`、`DriverUnavailable { hint }`、`Internal`。
- 保留数据库的原生错误码和信息，如 `ORA-00942` 和达梦的错误码。
- 不吞错误：每个错误要么展示给用户，要么写进日志。日志中不得出现密码或令牌。

## 9. 测试

- `dblens-dialect`：单元测试，加上 DDL 和元数据 SQL 的快照测试（`insta`）。
- 驱动层：用 Docker 起 MySQL、MariaDB、PostgreSQL、openGauss、Oracle Free、DM8。所有驱动跑同一套一致性测试：
  - 每种类型的读写往返；
  - 取消；
  - 事务；
  - 多结果集；
  - 错误码；
  - LOB 预览。
- `dblens-service` 和 `dblens-api`：集成测试，用真实的 MySQL 和 PG 容器。
- 前端：组件测试（Vitest），加上基于浏览器模式的端到端测试（Playwright）。
- CI：GitHub Actions 在 Windows、macOS、Linux 上构建和测试。Oracle 和达梦的容器测试只在 Linux 上运行。

## 10. 打包、发布与更新

| 平台 | 产物 |
|---|---|
| Windows x64 | NSIS 的 exe 安装包，默认按当前用户安装（不需要管理员权限）。不提供免安装版 |
| macOS | dmg（Intel 和 Apple Silicon，或通用包） |
| Linux x64 | deb、rpm、AppImage |
| 信创 ARM64 | deb（适配统信 UOS、银河麒麟） |
| 信创 LoongArch | deb，尽力支持；需要在龙芯机器或 QEMU 上构建，发布阶段再落实 |
| 浏览器模式 | 各平台单独提供 `dblens-server` 可执行文件 |

- **代码签名**：不购买 Windows 代码签名证书。首次运行时 SmartScreen 会提示"未知发布者"，需要用户手动放行。macOS 在没有开发者账号时，同样需要用户手动放行。
- **自动更新**：用 Tauri updater 插件。
  - 更新包用 updater 自带的密钥对签名（免费，与代码签名证书无关）。私钥必须离线备份，丢失后已发布的版本将无法再收到更新。
  - 更新源是一个静态的 `latest.json`，可以放在 GitHub Releases 或任意静态文件服务器上（便于国内网络和内网部署）。
  - 流程：启动后在后台检查 → 提示新版本和更新说明 → 用户确认 → 下载、校验、安装 → 重启。设置里可以关闭自动检查。
  - Linux 上只有 AppImage 支持自动更新；deb、rpm 和信创包只提示新版本。
  - `dblens-server` 只提示有新版本，不会自动替换自己。

## 11. M2–M5 边界

| 里程碑 | 内容 | 依赖的地基 |
|---|---|---|
| M2 数据 | 表数据网格编辑：改动暂存，生成的 DML 预览后在一个事务里提交。没有主键或唯一键的表只读。CSV/Excel 导入：列映射、类型校验、分批提交、出错时跳过或停止。表或查询结果导出 | 驱动、流式结果、方言的 DML 生成、Policy |
| M3 结构设计 | 表设计器：列、索引、约束、注释、分区（只读）。编辑后生成变更列表，再生成 DDL，预览后执行 | 结构模型、方言的 DDL 生成 |
| M4 模型 | YAML 模型文件（可放进 git）、从库反向生成模型、三层比对（表 → 列 → 约束和索引，带改名检测）、同步脚本、ER 图（按列画外键连线，支持自定义逻辑外键）、数据字典和设计文档导出（docx/html/markdown，可选模板） | 结构模型、方言 |
| M5 迁移 | 选择源和目标 → 按逻辑类型映射结构，生成目标 DDL，可人工调整 → 分批迁移数据（断点续传、进度、行数校验） → 最后建索引和约束 | M2 的批量读写、M3 的 DDL 生成、类型映射 |
| 后续 | 团队管控（多用户、RBAC、SQL 审核、审计、执行前备份）、JDBC 桥、SQLite、SQL Server 等 | 浏览器模式、Policy、Driver 接口 |

进度：
- 2026-10-26 M2 拆成两份（用户确认）：M2a 表数据编辑；M2b 导入（CSV / Excel 导入到表、SQL 文件导入）和对象树"导出整表"的入口。查询结果和数据标签的导出已在 M1c3 完成。
- M2a 已完成（设计 `docs/superpowers/specs/2026-10-26-m2a-data-editing-design.md`，详细计划 `docs/superpowers/plans/2026-10-26-m2a-data-editing.md`，说明 `docs/notes/m2a-data-editing.md`）。
- M2b 已完成（设计 `docs/superpowers/specs/2026-10-27-m2b-import-design.md`，详细计划 `docs/superpowers/plans/2026-10-27-m2b-import.md`，说明 `docs/notes/m2b-import.md`），M2 完成。
- M3 已完成（设计 `docs/superpowers/specs/2026-10-27-m3-table-designer-design.md`，详细计划 `docs/superpowers/plans/2026-10-27-m3-table-designer.md`，说明 `docs/notes/m3-table-designer.md`）。
- 2026-10-27 用户决定：国产库适配（OceanBase、人大金仓、PolarDB、TiDB、TDSQL、GaussDB、GoldenDB、GBase）作为 M3 之后的单独里程碑，分三批：有 Docker 或免费版的；只有云服务的；需要新驱动或新架构的（GBase 8s、OceanBase 的 Oracle 模式）。
- 国产库第一批已完成：TiDB、OceanBase 社区版（MySQL 模式）、openGauss（A、PG 兼容模式，含 SHA256 认证）、人大金仓（Oracle、PG 模式）（设计 `docs/superpowers/specs/2026-10-27-domestic-batch1-design.md`，详细计划 `docs/superpowers/plans/2026-10-27-domestic-batch1.md`，说明 `docs/notes/domestic-batch1.md`）。第二批（PolarDB、TDSQL、GaussDB，只有云服务）、第三批（GBase 8s、OceanBase 的 Oracle 模式）尚未开始；GoldenDB 没有可用的测试环境。

## 12. 风险与待验证项

| 风险 | 应对 |
|---|---|
| 达梦 DPI 的 FFI 绑定工作量和稳定性没有把握 | M0 先做一个技术验证：连接、查询、取消、取 LOB。不顺利就改走 ODBC |
| `oracle` crate 在各平台上加载 Instant Client 的路径问题 | 在 M0 的驱动验证中覆盖 Windows、macOS、Linux 和 ARM64 |
| LoongArch 上 Tauri/WebKitGTK 的可用性 | 列为尽力支持，发布阶段再验证 |
| 大结果集在前端的内存占用 | 设单结果集行数上限，并引导用户导出 |

## 13. 不在首期范围

- 图形化执行计划、SQL 调试器、存储过程编辑和调试
- 分区、触发器、存储过程的可视化编辑（首期只读）
- 多用户、权限与审计（只预留 Policy 接口）
- SQLite、SQL Server 以及其他国产数据库（虚谷、GBase、神通等）
