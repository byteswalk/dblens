# M0-3a 遗留事项

M0-3a（方言核心）在逐任务审查和全分支终审中，有一部分问题判定为不阻塞合并，记录在这里。写 M0-3b / M0-4 / M1 / M5 的计划时逐条处理，或明确说明不做。

## M0-3b 之前（M0-3b 未处理，没有实例可测）

M0-3b 已完成：Dialect 带服务器版本、识别 PolarDB、按真实服务器校验保留字表、在 DM8 上逐项验证达梦行为。验证结论见 `m0-3b-metadata.md`。

- Oracle 11g 的 ROWNUM 分页：`Dialect` 已经有版本号，但 `paginate` 仍只支持 12c 以上的 `OFFSET … FETCH`。
- 金仓、openGauss、GaussDB 在 Oracle 兼容模式下，不加引号的标识符会转成大写，需要按变体覆盖 `ident` 的规则。
- Oracle 不允许标识符里出现 `"`，即使写成两个 `""` 也不行。目前生成的语句会执行失败，不会出现注入，但应该在生成前就报错。
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
  - Oracle 浮点：`1e300`、`5e-324` 这样不带后缀的写法会按 NUMBER 解析，超出范围时上溢报错或下溢成 0；应加 `d` 后缀或写成 `TO_BINARY_DOUBLE(…)`；
  - `-0.0` 写出时丢负号；
  - 带指数的 Decimal 文本（如 `1.0E+30`）在 MySQL 中会按 DOUBLE 解析，丢精度；
  - Oracle 的空二进制写成 `HEXTORAW('')`，在 Oracle 中等于 NULL；
  - 时间类：小数秒最多写 9 位（MySQL 只保留 6 位并四舍五入）；年份不在 1..9999 时写不出合法字面量；Oracle 的 RAW(16) 列不能接收带横线的 UUID 文本；Interval 按文本原样写出，跨库不通用；
  - 生成 SQL*Plus / DIsql 脚本时要加 `SET DEFINE OFF`（字符串里的 `&`）；负数字面量不能紧跟在 `-` 后面（`--7` 会变成注释）。
- Oracle 的 `VARCHAR2(n CHAR)` 在 n≤4000 时保留。多字节值超过 4000 字节时插入会报 ORA-12899，这一点已接受。

## M0-3b 新增

- 元数据查询结果中，CLOB/LONG 文本超过驱动 64 KiB 预览上限时会变成 `Value::Lob` 预览，组装出的视图定义、程序源码会被截断。M1 实现"整体获取 LOB"后，服务层取源码时应改用完整获取。
- MariaDB 序列的参数（起始值、步长等）只能对每个序列单独查询，`sequences` 目前只返回名字；Oracle 的序列没有起始值。
- Oracle 整个 schema 的表明细在测试虚拟机上冷启动约 4 秒、服务器缓存热后约 0.2–0.5 秒（tables、indexes 最慢）。M0-4 的元数据缓存要避免重复取，必要时再优化这两条查询。
- Oracle 的 `V$RESERVED_WORDS` 需要 SELECT_CATALOG_ROLE，只在测试中用到；普通用户执行会报 ORA-00942。
- 达梦用 SQL 拿不到完整的版本号，只按主版本 8 处理。
- MySQL/MariaDB 的 `routine_source` 是由目录拼出的 CREATE 语句，不含 DEFINER 和字符集子句；参数列表受 `group_concat_max_len`（默认 1024）限制。
- 触发器定义在 MySQL、PG、达梦中是完整的 CREATE 语句，在 Oracle 中只有触发器体。
- PG 部分索引的 WHERE 条件、索引的 INCLUDE 列、MySQL 前缀索引的长度，在结构模型中都还没有位置。M4 做结构比对前要补上。
- 达梦空 INSERT 用 `DEFAULT VALUES`，表中有虚拟列时会失败（达梦不允许对虚拟列 INSERT）。
