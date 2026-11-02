# GoldenDB

中兴 GoldenDB，测试环境 V6.1.03.12SP5，内核版本号 8.9.99，两个分片。服务器报告的产品是 MySQL。JDBC 连接在版本号前补上产品名，由此识别出 `Variant::GoldenDb`，家族仍是 `Family::MySql`。

## 为什么走官方 JDBC 驱动

标准 MySQL 客户端有两个问题：

- 列定义包里的 catalog 不是 `def`，`mysql_common` 校验不过，连接就断。
- 服务端预处理语句经过代理后结果不对：同一条语句字面量能跑，绑定参数就报 `10613 Table doesn't exist` 或 `Unknown column 's.EXPRESSION'`。

官方驱动 `com.goldendb.jdbc.Driver`（MySQL Connector/J 5.1 的分支）在客户端绑定参数，这两个问题都没有。所以走 JDBC 桥接：`driver=mysql`、`client=jdbc`、`jdbc_kind=goldendb`，产品 `goldendb`。

驱动 jar 不随包发布，许可证没有确认过，也不在 Maven Central。用户把 `gdb-mysql-connector-java-*.jar` 放进 JDBC 驱动目录，文件名前缀是 `gdb`。

URL 上固定带的设置：

| 设置 | 原因 |
|---|---|
| `logger=…NullLogger` | 驱动默认把 SQL 原文写进日志 |
| `defaultFetchSize=-2147483648` | 流式读取。不用游标：`useCursorFetch` 要走服务端预处理 |
| `clobberStreamingResults=true` | 取消后流还开着，不加这个下一条语句会被驱动拒绝 |
| `tinyInt1isBit=false` | `TINYINT(1)` 按数字读 |

## 和 MySQL 不同的地方

Oracle 模式租户开 Oracle 兼容（`sql_mode` 含 `ORA_COMPATIBLE_MODE`、`ANSI_QUOTES`、`NO_BACKSLASH_ESCAPES`、`PIPES_AS_CONCAT`、`EMPTY_STRING_IS_NULL`）。

| 现象 | 处理 |
|---|---|
| `user` 是保留字 | 运维模板的别名加反引号 |
| `LIKE '%a\_b%'` 里的反斜杠不是转义符 | 模板改用 `ESCAPE '!'`。真 MySQL 开 `NO_BACKSLASH_ESCAPES` 时同样受影响 |
| 未加引号的别名变大写 | 元数据按列名取值时不分大小写，不受影响；导出的列名是大写 |
| 不能 `USE information_schema` | 服务器原文报错 |
| 大的文本表达式（`REPEAT` 等）标成 `MEDIUMBLOB` | 按二进制显示，`CONVERT(… USING utf8mb4)` 后才是文本。表里的 `TEXT` 列正常 |
| `information_schema` 把 `DATE` 列报成 `datetime` | `SHOW CREATE TABLE` 是 `date`。设计器和结构对比看到的是 `datetime` |
| `information_schema.TRIGGERS` 看不到用户触发器 | 模板用 `SHOW TRIGGERS`，`Dialect::adapt_rows` 换成标准的列；名字是大写的，按 `lower_case_table_names=1` 转小写 |
| 语法错误信息后面追加 `LinkInfo:` 地址 | 去掉后显示 |

## MySQL 模式租户

租户模式由向导第 2 步的基线模板决定，`mysql_cluster.json` 是 MySQL 模式。`sql_mode` 只有 `STRICT_TRANS_TABLES`，别名不变大写，`DATE` 列正常，`lower_case_table_names` 也是 1。服务端预处理、回收站、`ON DUPLICATE KEY UPDATE` 被拒绝这几点和 Oracle 模式相同。

**读不到触发器**：`SHOW TRIGGERS` 和 `information_schema.TRIGGERS` 都是空的，只有按名字 `SHOW CREATE TRIGGER` 才看得到。`super` 和对库有全部权限的普通账号结果相同，是租户本身的行为。所以重建表时触发器会丢失，预览里有警告（`design.rebuild.triggers_unknown`）。

`super` 对 `sys` 视图没有权限，所以 `locks` 改查 `performance_schema` 和 `information_schema.INNODB_TRX`，两种租户通用。

## 不支持或有风险

- **按主键更新的导入**：`ON DUPLICATE KEY UPDATE` 被代理拒绝（`12071`，要求 `SW`），报服务器原文。`REPLACE INTO` 能用，但语义不同，没有替换。
- **回收站**：`recyclebin=ON` 时 `DROP TABLE` 是移进回收站，表还占着 CHECK 约束名。同名约束的表再删或再建报 `3822`。重建带 CHECK 的表第二次会在删旧表这步失败，数据不受影响，新表 `*_dblens_new` 留着。可以先 `ALTER TABLE … DROP CHECK` 再删。
- **删库**：集群配置 `drop_database_enabled` 默认关。
- **取消**：只验证过 `SLEEP`。三表笛卡尔积取消后 8 秒内没结束，共用机器上不要用这种语句测试。

## 已验证

实库：GoldenDB V6.1.03.12SP5，经公网 8880 连接实例，只用 `dblens_test` 库。

- 服务层 11 个流程：完整流程、工作台、取消、对象、运维、导出、数据编辑、导入、SQL 文件、表设计器、模型。表设计器重建后触发器保留并生效；导入按上面的限制断言。
- 规范检查的存储信息查询。
- 迁移：MySQL → 这里，这里 → MySQL。
- MySQL 模式租户：同样的 11 个流程和双向迁移，触发器除外。
- 回归：MySQL、MariaDB、TiDB、OceanBase 的完整流程和表设计器。
- 真实鼠标（浏览器模式）：新建连接表单、保存、对象树、执行查询，连的是 MySQL 模式租户。系统库（`_gdb_*`、`dbagent`、`heartbeat_info`、`recyclebin`）归在“系统”下。

## 没有验证

- 账号权限：要建用户，这是共用集群。
- 分布式事务、分片键相关的写法。表默认是 `DISTRIBUTED BY DUPLICATE`（复制表）。
- 大结果集的流式读取和内存占用。
- `lower_case_table_names=0` 的集群：触发器的名字可能和表名大小写对不上。
- 真实锁等待时 `locks` 的结果和杀会话：`KILL` 的线程号是数据节点的，经代理未必能用。
