# 元数据查询：各家族的实现要点

M0-3b 的元数据查询模板在 `crates/dblens-dialect/sql/` 下，每个家族一个文件，兼容库可以另有覆盖文件（目前只有 `mariadb.sql`）。每个查询都返回同一套规范化的列，由 `meta::assemble` 统一组装成结构模型。

本文记录写模板时在真实数据库上确认的事实和取舍。实测版本：MySQL 9.7.4、MariaDB 12.3.3、PostgreSQL 18.6、Oracle 23ai Free（23.26.3）、DM8（V8，DB Version 0x7000d）。

## 通用约定

- **参数：** 模板里不能输出参数值，只能用 `{{ bind("schema") }}` 生成占位符，参数一律绑定。
- **过滤框：** 用户输入转成 `%…%` 的 LIKE 模式，以 `!` 作转义符，按名字或注释不区分大小写匹配。
  - MySQL 的视图没有注释，Oracle 和达梦的序列、程序、同义词也没有，这些对象只按名字匹配。
- **批量：** 一个 schema 的表、列、约束、检查约束、索引、触发器各用一条 SQL 取回，共六条，见 `MetaKey::TABLE_DETAIL`；给出 `table` 时只取这一张表。
- **索引：** 支撑主键和唯一约束的索引不出现在 `indexes` 中，它们已经在 `keys` 里了。
- **版本号：** 编码为 `major*10000 + minor*100 + patch`。版本未知时按最新版本渲染。
- **只测过最新版本：** 旧版本分支只验证了能渲染，没有在旧版本数据库上实际执行过。具体分支：
  - MySQL 低于 8.0.13 和 8.0.16 的分支；
  - PG 低于 11 和 12 的分支；
  - Oracle 低于 12.1 和 12.2 的分支。

## MySQL / MariaDB

- **schema：** 就是 database。
- **默认值：** MySQL 返回的字面量默认值不带引号，模板里重新拼成可以直接写进 DDL 的文本，如 `'无名'`。MariaDB 返回的已经是 SQL 文本。
- **版本要求：**
  - 检查约束要 MySQL 8.0.16 以上，更早的版本返回空结果；
  - 函数索引表达式要 8.0.13 以上。
- **MariaDB：**
  - JSON 列是带 `json_valid` 检查约束的 `longtext`。这条隐式的检查约束已过滤，类型映射为 Text。
  - 序列只能列出名字，参数要对每个序列单独执行 `SELECT … FROM 库.序列`，留给服务层。
  - `information_schema.KEYWORDS` 不区分是否保留，所以 MariaDB 没有 `reserved_words` 查询。
- **触发器：** 每个触发器只有一个事件。`definition` 是重新拼出的 `CREATE TRIGGER`，不含 DEFINER。
- **`routine_source`：** 由 `ROUTINES` 和 `PARAMETERS` 拼出完整的 CREATE 语句，不含字符集子句和 DEFINER。
  - 参数列表受 `group_concat_max_len` 限制，默认 1024。
  - 假定 sql_mode 没有开启 NO_BACKSLASH_ESCAPES。
- **索引：** 前缀索引只显示列名，不可见索引没有标记。

## PostgreSQL

- **数据来源：** 只用 `pg_catalog`。`information_schema` 会隐藏无权限的对象，而且慢。
- **自增：** 标识列，或者 `nextval()` 默认值所用的序列归属于该列（serial），都算自增。这类自带的序列不在 `sequences` 中列出。
- **分区：** 只列分区父表，`partition` 来自 `pg_get_partkeydef`。
- **函数：** 重载的函数在列表里合成一行，`routine_source` 返回全部重载的定义。聚合函数和窗口函数不列出。
- **检查约束：** 只取 `contype = 'c'`，PG 18 的 NOT NULL 约束（`'n'`）自然排除。
- **索引：** 只返回键列，INCLUDE 列和部分索引的 WHERE 条件不返回。
- **不支持的对象：** 外部表不列出。
- **兼容库的已知差异（没有实例，待验证）：**

  | 兼容库 | 差异 | 需要的覆盖 |
  |---|---|---|
  | openGauss | 没有 `relispartition`、`pg_get_partkeydef`、`relkind 'p'`，分区信息在 `pg_partition` 中 | 改用 `pg_partition` |
  | openGauss、9.x 内核的金仓 | 没有 `pg_sequence` | 序列参数只能逐个查询 |
  | openGauss | 版本号报告为 9.2，`prokind` 不可用；包在 `pg_package` 中 | 补上包的查询 |
  | openGauss | 没有 `attidentity`；生成列用 `pg_attrdef.adgencol` | 改写 `columns` |
  | openGauss、老版本金仓 | 没有 `indnkeyatts` | 改用 `indnatts` |
  | openGauss | 有全局临时表，`relpersistence = 'g'` | 一并排除 |
  | openGauss、金仓 | 额外的系统 schema：openGauss 的 `dbe_*`、`snapshot`、`cstore`、`pkg_service`，金仓的 `sys` | 加进 `is_system` 的判断 |

## Oracle

- **数据来源：** 用 `ALL_*` 视图，不用需要额外权限的 `DBA_*`。
- **LONG 列：** 驱动把 LONG 列（`DATA_DEFAULT`、`ALL_VIEWS.TEXT`、`TRIGGER_BODY`、`SEARCH_CONDITION`）完整地读成字符串。
  - `*_VC` 列会在 4000 字符处悄悄截断，只用在 WHERE 条件里，不作为结果返回。
  - LONG 不能经过 CASE、函数或 LIKE，所以 `columns` 用 `ALL_TAB_COLS` 自连接三次，把默认值和虚拟列表达式分开取。
- **尾部空白：** 默认值、表达式、定义末尾带空白，组装时统一去掉。
- **NOT NULL：** 12.2 以前，NOT NULL 也以 `"COL" IS NOT NULL` 形式的检查约束出现，由模板和组装两处过滤。
- **函数索引：** 表达式在 12.2 以上通过 `DATA_DEFAULT_VC` 取得。更早的版本另返回一个 LONG 列 `column_expression`，组装时优先使用它。降序索引列 `"CREATED"` 会去掉引号。
- **外键：** 没有 ON UPDATE 动作，`on_update` 恒为 NULL。`on_delete` 可能是 `NO ACTION`，但 Oracle 的 DDL 写不出这个子句，M3 生成 DDL 时要省略。
- **序列：** `ALL_SEQUENCES` 没有起始值，`start_value` 恒为 NULL。标识列自带的 `ISEQ$$` 序列不列出。
- **触发器：** `definition` 只有触发器体，触发器头与 LONG 列无法在 SQL 中拼接。
- **保留字：** 取 `V$RESERVED_WORDS` 中 RESERVED、RES_SEMI、RES_ATTR 任一为 Y 的词。
  - 只看 RESERVED 会漏掉 ACCESS、COMMENT、LEVEL、USER。
  - 这个视图需要 SELECT_CATALOG_ROLE，普通用户会报 ORA-00942。它只用于测试，不影响正常功能。
- **性能（较慢的虚拟机）：** 整个 schema 的表明细冷启动约 4 秒、服务器缓存热后约 0.2–0.5 秒；SYS 下 13 万列用时 2.7 秒。M0-4 做元数据缓存时要考虑这一点。

## 达梦 DM8

- **版本号：** 用 SQL 拿不到 8.1.x 这样的完整版本，`V$INSTANCE` 只有 `DB Version 0x7000d` 和构建号，所以只取主版本 8。目前没有模板需要按版本分支。
- **schema：** 来自 `SYSOBJECTS TYPE$='SCH'`。`ALL_USERS` 不包含 CTISYS 这类没有对应用户的模式。
- **表：** 排除临时表（其中包括自引用外键产生的 `SREF_CON_TAB*`）、物化视图存储表 `MTAB$_*` 和位图索引存储表 `BM$_*`。
- **列：**
  - 标识列来自 `SYSCOLUMNS.INFO2` 的标志位。
  - 虚拟列的表达式在 `DATA_DEFAULT` 中。
  - 列类型由目录列重新拼出，长度单位总是显式写出 `(n CHAR)` / `(n BYTE)`；全部 79 种类型都能原样建表、读回。
- **序列：** 起始值和步长来自 `SYSOBJECTS` 的 INFO3、INFO4，因为 `ALL_SEQUENCES` 没有起始值。
- **索引：** 只取 `GENERATED='N'` 的索引，全文索引因此不列出。函数索引的表达式来自 `ALL_IND_EXPRESSIONS`。
- **视图定义：** 是达梦重新生成的文本（大写，多余空格）。
- **程序源码：** `ALL_SOURCE` 中整段只有一行，就是完整的 `CREATE OR REPLACE` 文本。
- **保留字：** `V$RESERVED_WORDS` 中 RESERVED 为 Y 的词，再加上不能直接作列名的伪列 ROWID、TRXID、VERSIONS_*。其中达梦专有的词另列在 `ident.rs` 的 `DAMENG_RESERVED` 中，只对达梦加引号。
- **已确认的行为（M0-3a 时的推测，这次在实例上逐项验证）：**

  | 行为 | 结论 |
  |---|---|
  | `VARCHAR(n CHAR)` / `CHAR(n CHAR)` | 支持 |
  | 本实例的 LENGTH_IN_CHAR | 0，按字节计长，`VARCHAR(3)` 存不下 '中文字'，所以生成 DDL 时必须写 `CHAR` |
  | 不带精度的 `DEC` / `NUMBER` | 精度和标度都不限，1.55 原样读回 |
  | `DEC(65,30)` | 不支持，精度上限 38 |
  | `DEC(5,-2)`、标度大于精度 | 都不支持 |
  | `TIMESTAMP(9)` | 可以建，但只存 6 位小数 |
  | VARCHAR 长度上限 | 随页大小变化，32K 页时为 32767；方言按 8 KB 页的 8188 保守处理 |
  | 字符串字面量长度 | 到 10 万字符仍正常；`TO_CLOB` 拼接也可以用 |
  | `q'[...]'` 引号、`0x` 二进制字面量 | 都支持 |
  | `''` | 不是 NULL，是空串（与 Oracle 不同） |
  | NaN、Infinity | 都不支持，字面量生成时报错 |
  | 在末尾追加 `LIMIT … OFFSET`、`OFFSET … FETCH` | 都支持 |
  | `INSERT … (标识列) VALUES (DEFAULT)` | 不支持，报错 -2871；没有可插入的列时改用 `INSERT … DEFAULT VALUES`，表中有虚拟列时它也会失败 |
  | `SET SCHEMA` | 可用，驱动用它处理连接参数中的 database |

## 达梦驱动的并发加载死锁（已修复）

- **现象：** 达梦连接和 PostgreSQL 连接同时发起时，整个进程卡死。用 cdb 抓到的线程栈显示两边互相等待：
  - dmdpi 的 DLL 初始化例程在持有加载器锁的情况下调用 `getaddrinfo`，等待 Winsock 的目录锁；
  - 另一个线程正在第一次创建 socket，持有 Winsock 的目录锁，等待加载器锁以加载协议提供程序 DLL。
- **修复：** 加载 dmdpi 之前，先在本线程初始化 Winsock，见 `load.rs` 的 `warm_up_winsock`。
- **回归测试：** `crates/dblens-driver-dameng/tests/concurrent_load.rs`。
