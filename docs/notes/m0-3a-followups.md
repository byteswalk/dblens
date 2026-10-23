# M0-3a 遗留事项

M0-3a（方言核心）在逐任务审查和全分支终审中，有一部分问题判定为不阻塞合并，记录在这里。写 M0-3b / M0-4 / M1 / M5 的计划时逐条处理，或明确说明不做。

## M0-3b 之前（写进 M0-3b 计划）

- `Dialect` 只保存变体，没有服务器版本。设计文档 6.3 节要求识别"具体产品和版本"，元数据模板也需要按版本分支，例如：
  - MySQL 8.0.16 起才有 `CHECK_CONSTRAINTS`，MariaDB 是 10.2 起；
  - PG 11 起才有 `pg_proc.prokind`；
  - Oracle 12c 起才有标识列，23ai 起才有 BOOLEAN；
  - Oracle 11g 的 ROWNUM 分页也依赖版本（目前 `paginate` 只支持 12c 以上的 `OFFSET … FETCH`）。
- PolarDB 目前识别不出来（MySQL 驱动把 `product` 写死为 "MySQL"），需要额外查询 `@@version_comment` 或 `polardb_version` 变量。
- 保留字表需要对照真实服务器校验：每个服务器报告的保留字都必须在表里。
  - MySQL：`information_schema.KEYWORDS WHERE RESERVED=1`
  - PG：`pg_get_keywords()` 中 catcode 为 R 或 T 的词
  - Oracle 和达梦：`V$RESERVED_WORDS WHERE RESERVED='Y'`
- 达梦行为待验证（M0-3a 时测试虚拟机未开，以下都是按文档或推测实现的）：
  - `VARCHAR(n CHAR)` 和 `CHAR(n CHAR)` 语法，以及 `LENGTH_IN_CHAR` 的默认值；
  - 不带精度的 `DEC` 的默认精度和标度（若标度默认为 0，溢出回退成 `DEC` 会丢小数，可改为 `DEC(38,min(s,37))`）；
  - 字符串字面量的长度上限，以及是否需要像 Oracle 一样拆成 `TO_CLOB` 分块；
  - `q'[...]'` 引号、`0x…` 二进制字面量、`''` 是否等于 NULL；
  - `TIMESTAMP '… +08:00'`、`TIME '…'`、`1e300` 这几种字面量；
  - 末尾追加 `LIMIT … OFFSET …` 的分页写法；
  - VARCHAR 最大长度 8188 是否随页大小变化。
- 金仓、openGauss 在 Oracle 兼容模式下，`decode(…,'hex')` 可能和 Oracle 的 `DECODE` 函数重名冲突，需要实测；冲突的话改用 `E'\\x…'::bytea`。

## M0-4 之前

- `Error` 缺少表示"数据本身不合法"的变体：字面量生成失败（NaN、含 NUL 的字符串、超长二进制）目前都报 `Internal`。API 层做错误映射时应补一个合适的变体。
- `SqlStatement` 没有实现 `Serialize`；偏移量是 UTF-8 字节偏移，而 Monaco 用 UTF-16 偏移或行列号，中文脚本的位置会对不上。需要提供转换。

## M1 之前

- `paginate` 只用于工具自己生成的浏览表数据的查询。它把分页子句追加在末尾，原查询已带 `LIMIT` / `FETCH` / `FOR UPDATE` 时会语法报错。编辑器里用户写的 SQL 靠执行流分批取数，不能交给 `paginate` 改写。
- 数据编辑需要一个通用的行定位键解析：优先主键；没有主键时，取第一个所有列都 NOT NULL 的唯一约束；都没有则不能按行编辑。可空的唯一列不能用（`col = ?` 匹配不到 NULL）。函数索引和部分索引不能用作唯一键。
- 语句拆分的已知限制（详见 `split.rs` 模块文档）：
  - PL/SQL 块缺少 `/` 时会吞掉后面的语句；
  - 不支持 PG 的 `BEGIN ATOMIC`；
  - MySQL 存储程序必须配合 `DELIMITER` 使用；
  - 不识别 SQL*Plus 命令（`SET`、`PROMPT`、`@` 等）；
  - `/` 后面跟注释时不算结束符。

  如果用户反馈多，再考虑加启发式。

## M5（数据迁移）之前

- 带时区时间写入 MySQL 的约定要统一：目前字面量和 MySQL 驱动都按"值自身时区的本地时间"写入 DATETIME，跨时区来源（Oracle、达梦）的值会混在一列里。要决定是否统一换算成 UTC，并在字面量、驱动绑定和迁移层三处保持一致。
- 类型映射：
  - Oracle `NUMBER(p)` / `NUMBER(p,0)` 一律映射为 Decimal，整数列迁移后会变成 `NUMERIC(10)` 之类，可以考虑 p≤18 时映射为整数类型；
  - MySQL 不带参数的 `decimal` 默认精度是 (10,0)，目前映射为不限精度（从 information_schema 读取时总带参数，实际碰不到）；
  - Oracle 的 NCHAR / NVARCHAR2 与 BYTE、CHAR 长度语义的区别丢失了；
  - 达梦缺少 NCHAR、NVARCHAR、BINARY_DOUBLE 的映射；
  - MySQL 的 `tinyint(1)` 当作布尔，0 和 1 以外的值会被强制转换；
  - PG 的 `json` 映射为 `JSONB`，会丢键的顺序和重复键；
  - 参数溢出（如 `number(4294967296)`）会被悄悄当作不限精度。
- 字面量：
  - `-0.0` 写出时丢负号；
  - 带指数的 Decimal 文本（如 `1.0E+30`）在 MySQL 中会按 DOUBLE 解析，丢精度；
  - Oracle 的空二进制写成 `HEXTORAW('')`，在 Oracle 中等于 NULL；
  - 时间类：小数秒最多写 9 位（MySQL 只保留 6 位并四舍五入）；年份不在 1..9999 时写不出合法字面量；Oracle 的 RAW(16) 列不能接收带横线的 UUID 文本；Interval 按文本原样写出，跨库不通用；
  - 生成 SQL*Plus / DIsql 脚本时要加 `SET DEFINE OFF`（字符串里的 `&`）；负数字面量不能紧跟在 `-` 后面（`--7` 会变成注释）。
- Oracle 的 `VARCHAR2(n CHAR)` 在 n≤4000 时保留。多字节值超过 4000 字节时插入会报 ORA-12899，这一点已接受。
