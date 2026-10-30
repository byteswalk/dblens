# M8a GBase 8s：连接、浏览、查询 设计

日期：2026-10-30

## 1. 目标

支持 GBase 8s（Informix 系，SQLI 协议）：连接、按"连接 → 库 → 属主 → 对象"浏览、执行 SQL 和过程脚本、编辑结果、运维查询、生产环境危险语句确认。

表设计器、导入、文档导出、结构比对、保存为模型、迁移、规范检查留给 M8b：后端报 `gbase.later`，前端入口置灰并提示，迁移和比对的来源、目标列表不出现 GBase 8s。

## 2. 范围

- 实测版本：GBase 8s V8.8_3.6.5（VM .106，容器 `gbase8s`，端口 9088，服务器名 `gbase01`，库 `dblens_test`）。
- 只做 8s。GBase 8a、8c、8t 是别的产品，不在范围内。
- 表单只列 GBase 8s。驱动叫 `informix`，因为协议来自 Informix；IBM Informix 未实测，不列入产品，也不做产品识别。
- 不用 CSDK、ODBC，不自己实现 SQLI 协议。
- Oracle、达梦的现有驱动不动。桥接在 GBase 上稳定后，再单独评估 Oracle 换 ojdbc。

## 3. JDBC 实测结论

2026-10-30 用下面的驱动在 JDK 25、Windows 上连 VM 实例实测。

- 驱动：官方 `gbasedbtjdbc_3.6.5_2L1_13.jar`，3.1 MB，类 `com.gbasedbt.jdbc.Driver`，SHA-256 `e555a00ed5f2f50fd890ed9bca07107bcf7477ce72102d9db4c723ddddf111ef`。
- URL：`jdbc:gbasedbt-sqli://<主机>:<端口>/<库>:GBASEDBTSERVER=<服务器名>;CLIENT_LOCALE=zh_CN.utf8;DB_LOCALE=zh_CN.utf8;SQLMODE=gbase`

| 项目 | 结果 |
|---|---|
| 连接 | 95–151 ms；不设 locale 时旧驱动报内部异常，新驱动的行为未单独测 |
| `SQLMODE` | 必须设为 `gbase`；不设时 `sysmaster:` 前缀和 `CREATE TEMP TABLE` 报 `-201` |
| 中文、生僻字 | `VARCHAR`、`LVARCHAR` 正常 |
| `BIGSERIAL`、`DECIMAL(20,6)`、`BOOLEAN`、`TEXT` | 正常；`DECIMAL` 取回 `BigDecimal`，精度完整 |
| `DATETIME YEAR TO FRACTION(3)` | 能读；`CURRENT` 取回的毫秒是 `.0`，待确认是服务器行为还是驱动截断 |
| 取消 | `Statement.cancel()` 后返回 `-213`，连接继续可用 |
| 事务 | `setAutoCommit`、`rollback` 正常 |
| `sysmaster` | `admin` 能读；`dblens_test` 有日志，locale 为 `zh_CN.57372` |
| 元数据 | 不用 `DatabaseMetaData`，查系统表（第 6 节） |

实例自带的 2015 年 `ifxjdbc.jar` 不可用：`VARCHAR` 中文读成空值，读 `TEXT` 卡死。

## 4. 桥接架构

```text
dblens（Rust）── stdin/stdout 帧 ──> dblens-bridge（Java 进程）── JDBC ──> GBase 8s
```

- **Rust：** 新 crate `dblens-driver-jdbc`，实现 `Driver`、`Connection`，`DriverId::Informix`。能力：`catalogs: true`、`schemas: true`、`transactional_ddl: true`、`cancel: true`、`explain: false`、`multiple_result_sets: false`。
- **Java：** 新目录 `bridge/`，没有第三方依赖，只用 JDK 自带类，构建产物 `dblens-bridge.jar`。构建 Java 部分只在打包时需要 JDK，`cargo` 构建不依赖它。
- **进程：** 全应用一个 Java 进程，第一次连 GBase 时才启动，不用 GBase 的用户没有额外开销。stdin 关闭时进程退出，所以主程序崩溃不会留下孤儿进程。
- **线程：** Java 端每条连接一个工作线程，读帧线程只负责分发，因此取消请求不会被运行中的查询挡住。
- **密码：** 只放在帧里，不进命令行和环境变量，不写日志。
- **崩溃：** Java 进程退出时，所有桥接连接报连接错误；下一次连接重新启动。stderr 写入 DBLens 日志。

### 4.1 帧协议

每帧：`u32 长度 + u64 请求号 + u8 类型 + 内容`，大端。值用带类型标记的二进制，覆盖 `Value` 的全部类型。不用 JSON，大结果集时编解码开销太高。

| 请求 | 响应 |
|---|---|
| `connect`（驱动类、URL、属性） | 连接号，或错误 |
| `execute`（连接号、SQL、参数） | 列信息，或影响行数 |
| `fetch`（连接号、行数） | 一批行和是否结束 |
| `cancel`、`commit`、`rollback`、`set_autocommit`、`ping`、`close` | 成功或错误 |

取数由 Rust 用 `fetch` 按批拉取，结果流背压与现有驱动一致，不会把大结果集堆在内存里。错误带 `SQLException` 的错误码和消息。

### 4.2 GBase 适配

- URL 由连接参数拼出，始终带 `GBASEDBTSERVER`、`CLIENT_LOCALE`、`DB_LOCALE`、`SQLMODE=gbase`、`DELIMIDENT=y`。URL 不进日志。
- `服务器名`（`gbase01`）必填，表单上单独一项。`DB_LOCALE` 默认 `zh_CN.utf8`，高级选项里可改。
- 认证失败码 `-951`、`-952` 转 `Error::Auth`，不重试；实际错误码在实现时核对。
- 无日志的库不支持事务。`set_autocommit(false)` 报错时把服务器的错误原样返回。
- 大字段（`BYTE`、`TEXT`、`BLOB`、`CLOB`）用流读取，超过 `lob_limit` 截断为 `Value::Lob`。

类型映射：

| GBase 8s | Value |
|---|---|
| `SMALLINT`、`INTEGER`、`INT8`、`BIGINT`、`SERIAL`、`BIGSERIAL` | 整数 |
| `SMALLFLOAT`、`FLOAT` | 浮点 |
| `DECIMAL`、`MONEY` | 按字符串，保留精度 |
| `CHAR`、`NCHAR`、`VARCHAR`、`NVARCHAR`、`LVARCHAR` | 文本 |
| `DATE`、`DATETIME` | 日期时间；精度不完整时（如 `HOUR TO SECOND`）按文本 |
| `BOOLEAN` | 布尔 |
| `INTERVAL` 和其他扩展类型 | 文本 |

## 5. 分发

用户不用自己安装 Java 或客户端。

| 部件 | 处理 |
|---|---|
| `dblens-bridge.jar` | 我们自己的代码，随包发布 |
| 精简 JRE | 打包时用 `jlink` 按平台生成，放在 `clients/jre/`，模块由 `jdeps` 确定，预计每平台 40–50 MB（未实测）。基于 OpenJDK，附 GPLv2+CE 声明 |
| `gbasedbtjdbc.jar` | 许可未确认，见下 |

查找 JDBC 驱动的顺序：连接选项 `jdbc_dir`（设置中指定后只查这一处）→ 程序自带的 `clients/jdbc/`。找不到时报 `DriverUnavailable`，提示官方下载地址和放置位置。不从工作目录加载。

- **打包：** 新增 `scripts/prepare-clients.mjs`，生成 JRE 并把清单里的文件复制到被 git 忽略的 `clients-staging/`，由 `tauri.bundle.json` 的 `resources` 打进安装包；`dblens-server` 的压缩包同样带上。
- **发布门槛：** 清单里的文件缺失时打包失败。
- **仓库：** 第三方 jar 和 JRE 绝不提交。公开仓库里只有脚本和清单（文件名、版本、SHA-256）。
- **声明：** 第三方许可汇总到 `THIRD-PARTY-NOTICES`。

**GBase jar 的许可：** jar 里没有许可文件，官网授权页没有再分发条款。在取得 GBase 书面确认前，它不进清单，首版由用户自己放置（官网免登录可下载）。确认后把它加入清单即可，代码不用改。达梦、Oracle 的内置不在 M8a 范围内。

## 6. 方言

新增 `Family::Informix`、`Variant::Gbase8s`（序列化为 `gbase8s`）。

- 标识符：双引号，未加引号时折叠为小写。
- 占位符 `?`；字面量 `'…'`，不用 `N` 前缀。
- 分页：`SELECT SKIP n FIRST m …`。浏览表数据与其他库相同，流式分批读取，不生成分页。
- 对象全名 `库:属主.表`；同库内写 `属主.表`。
- 脚本拆分：`;` 结尾；`CREATE PROCEDURE`、`CREATE FUNCTION` 到 `END PROCEDURE`、`END FUNCTION` 整体算一条；前后端同一规则。`{ … }` 是注释，和 `--`、`/* */` 一样处理。
- 危险语句：sqlparser 解析不了时按关键字扫描（`DROP`、`TRUNCATE`、`DELETE`、`UPDATE`）。
- 元数据模板 `sql/informix.sql`，查当前库的 `systables`、`syscolumns`、`sysindices`、`sysconstraints`、`sysviews`、`sysprocedures`、`systriggers`。`coltype` 和 `collength` 的解码（类型、可空、`DECIMAL` 精度、`DATETIME` 限定符）放在 Rust，有单元测试。
- 库列表查 `sysmaster:sysdatabases`。元数据连接池按"连接 + 库"区分（`Profile::in_catalog`），沿用 M6a。
- 运维模板 `sql/ops/informix.sql`，查 `sysmaster`：会话、运行中语句、锁、存储（dbspace）、大表。没有慢 SQL 来源时不显示这一项。
- 执行计划：`SET EXPLAIN` 写到服务器文件，M8a 不做，`explain: false`，入口隐藏。

## 7. 前端

- `products.ts`：加 `{ id: 'gbase8s', driver: 'informix', port: 9088 }`，放在国产库组；图标用 `dbIcons` 的现有机制。
- 连接表单：GBase 8s 显示"服务器名"（必填）和高级选项 `DB_LOCALE`、`JDBC 驱动目录`。
- 对象树：沿用 M6a 的库层；系统库 `sysmaster`、`sysadmin`、`sysuser`、`sysutils` 排在后面并标注。
- 编辑器：按通用 SQL 高亮和补全，补全取元数据；格式化用 sql-formatter 的通用方言。不新增语法文件。
- 词条中英文齐全；M8b 的入口用 `gbase.later`。

## 8. 测试

- 单元：帧编解码（Rust 和 Java 两端，用同一组样例）、URL 拼接、`coltype` 解码、脚本拆分、错误码归类。这些不依赖数据库。
- 一致性：`dblens-driver` 的 conformance，环境变量 `DBLENS_TEST_GBASE_URL`，jar 路径 `DBLENS_TEST_JDBC_DIR`；密码只内联传入。
- 桥接：Java 进程被杀后，连接报错且下一次连接恢复；大结果集分批读取时内存不随行数增长；取消运行中的查询。
- 元数据：`meta_snapshots` 和 `meta_live` 加 GBase 8s，夹具 `fixtures/gbase8s.sql`。
- 服务和接口：`dblens-service`、`dblens-api` 的 live 测试串行运行，不加 `--include-ignored`。
- 打包：Windows 安装包在没有系统 Java 的环境（Win11 VM .103）里连上实例；`dblens-server` 在 Linux 上连上实例。
- 界面：内置浏览器用真实鼠标走完连接、树、查询、编辑、危险语句确认、运维，再确认 jar 缺失时的提示。
