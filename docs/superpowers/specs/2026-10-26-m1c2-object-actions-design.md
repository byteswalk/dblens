# M1c2 对象操作 设计

日期：2026-10-26。上级设计：`docs/superpowers/specs/2026-10-25-m1-workbench-design.md`（3 节后端新增、4 节拆分）。M1c 的第二份（M1c1 历史与收藏已完成，M1c3 导出与运维查询在后）。

## 1. 目标与范围

对象树中的表、视图、物化视图、序列、函数 / 过程 / 包、同义词可以：
- **查看 DDL**：弹出只读预览；
- **生成 SQL**：SELECT（每列附注释）、INSERT、UPDATE、DELETE、关联查询，插入到当前编辑器；
- **查看数据**（表、视图、物化视图）：打开只读的数据标签，可过滤、排序、滚动加载、统计行数。

不做：
- 表数据编辑（M2）；
- 表设计器和 DDL 变更生成（M3）；
- 触发器、索引作为单独对象查看 DDL（包含在表的 DDL 中）。

## 2. 已定的决策（2026-10-26 与用户确认）

| 问题 | 结论 |
|---|---|
| 查看数据 | 独立的只读数据标签：WHERE 过滤、点列头排序、滚动加载、按需统计行数。M2 的编辑在它上面扩展 |
| DDL 和生成 SQL 的去处 | DDL 弹出只读预览（复制、在新标签中打开）；生成的 SQL 插入当前编辑器的光标处，没有标签时新开 |
| 数据标签取数 | 方案一：复用会话和流式读取，SELECT 由后端按方言生成 |
| 打开数据标签 | 右键"查看数据"，或在对象树中双击表 / 视图 / 物化视图 |
| 统计行数 | 手动按钮，不自动统计 |

## 3. 查看 DDL

接口：`GET /api/profiles/{id}/schemas/{schema}/ddl?kind=&name=`，返回 `{ "ddl": "…" }`。

- `kind`：`table`、`view`、`materialized_view`、`sequence`、`function`、`procedure`、`package`、`package_body`、`synonym`。
- 不支持的组合（如 MySQL 的 `package`）返回 `invalid`；对象不存在返回 `not_found`。

各方言的取法：

| 方言 | 取法 |
|---|---|
| MySQL 系（MySQL、MariaDB、TiDB、OceanBase、PolarDB） | `SHOW CREATE TABLE / VIEW / PROCEDURE / FUNCTION`；序列（MariaDB）用 `SHOW CREATE SEQUENCE`；其他类型不支持 |
| Oracle、达梦 | `DBMS_METADATA.GET_DDL(类型, 名称, schema)`。包的规范和包体分开取；同义词用 `SYNONYM` 类型 |
| PG 系（PostgreSQL、KingbaseES、openGauss、GaussDB、HighGo） | 由结构模型拼出（见下） |

PG 系的拼法：
- **表**：
  - `CREATE TABLE`：列（类型取 `native`，`NOT NULL`、`DEFAULT`、生成列）、主键、唯一约束、外键（含 `ON DELETE / ON UPDATE`）、检查约束；
  - 之后依次为：非约束索引的 `CREATE [UNIQUE] INDEX … USING 方法 (列 [DESC])`；表和列的 `COMMENT ON`；分区信息（有时作为注释附上）。
- **视图、物化视图**：`CREATE [MATERIALIZED] VIEW 名 AS` + 已有的视图定义；有注释时附 `COMMENT ON`。
- **序列**：`CREATE SEQUENCE … START / INCREMENT / MINVALUE / MAXVALUE / CACHE / [NO] CYCLE`。
- **函数、过程**：已有的程序源码（`pg_get_functiondef`）。

DDL 的拼装放在 `dblens-dialect`（无 IO，单元测试），取数放在服务层；MySQL、Oracle、达梦的查询语句作为元数据模板加入 `dblens-dialect/sql/*.sql`，沿用现有写法。

界面：右键"查看 DDL"。
- 弹出只读的 Monaco 预览（按连接的语言高亮），标题为对象名；
- 按钮：复制、在新标签中打开（用该连接）、关闭；
- 取数时显示加载状态，出错时在预览中显示错误文字。

## 4. 生成 SQL

接口：`GET /api/profiles/{id}/schemas/{schema}/generate?table=&kind=&ref_schema=&ref_table=`，返回 `{ "sql": "…" }`。

`kind` 的取值：
- `select`：每列一行，附列注释（已有的 `select_sql`）；
- `insert` / `update` / `delete`：带占位符的模板（已有的 `insert_sql` / `update_sql` / `delete_sql`）。没有主键时 `update` / `delete` 返回 `invalid`，提示需要主键。
- `join`：关联查询。
  - 给出 `ref_table` 时，只连接与它相关的外键，两个方向都算：本表引用它，或它引用本表；
  - 不给时，连接本表引用的全部表。
  - 别名依次为 `t1`、`t2`…，`SELECT t1.*, t2.* … FROM 本表 t1 JOIN 被连接表 t2 ON t1.x = t2.y [AND …]`；
  - 同一个表被多个外键引用时，每个外键各连一次。

关联查询的生成放在 `dblens-dialect`（单元测试）。"引用本表的表"需要读取整个 schema 的外键（已有的 schema 级表明细接口，有缓存）。

界面：表和视图的右键菜单增加"生成 SQL"子菜单：
- SELECT、INSERT、UPDATE、DELETE（视图只有 SELECT）；
- "关联查询"子菜单：
  - "全部外键"；
  - 按外键列出相关的表，标明方向（"→ 父表"、"← 子表"）。
  - 没有外键时这一项置灰。
- 生成后插入到当前编辑器的光标处（沿用 M1c1 的插入通道）；没有打开的标签时用该连接新开一个。

## 5. 数据标签

**打开**：
- 右键表 / 视图 / 物化视图选"查看数据"，或在对象树中双击；
- 已经打开过同一对象的数据标签时，切换过去，不重复打开。

**标签**：
- 在标签列表中新增一种：`{ kind: 'data', profileId, schema, object, objectKind }`；
- 标题为对象名，图标与查询标签不同；
- 与查询标签一起保存、恢复，会话在第一次读取时打开。

**界面**：
- 工具栏：
  - 连接和 schema（只显示，不能改）；
  - WHERE 过滤框：输入条件后回车应用，清空即取消过滤；
  - 刷新、停止、"统计行数"；
  - 总行数（统计后显示）。
- 表格沿用查询标签的结果表格：首批 500 行，滚动续取，沿用行数上限，单元格可以查看和复制。
- 点列头排序：依次切换升序、降序、不排序；同一时间只按一列排序。
- 状态栏：连接、schema、已读取的行数。

**取数**：
- 前端把（对象、过滤、排序）交给后端：`POST /api/profiles/{id}/schemas/{schema}/data-sql`，请求体 `{ object, filter?, order? }`，返回按方言生成的 SQL：
  - `SELECT * FROM 限定名 [WHERE (过滤)] [ORDER BY 列 ASC|DESC]`；
  - 统计行数时另生成 `SELECT COUNT(*) FROM 限定名 [WHERE (过滤)]`（`count: true`）。
- 前端经数据标签自己的会话执行，沿用 `runner`（执行、续取、取消、重连）。
- 过滤条件原样放进括号中。由用户自己负责条件写法，出错时显示数据库的报错。

**其他**：
- 数据标签执行的 SELECT 照常记入历史（后端统一处理）。
- 只读：不生成任何修改语句；生产环境也不需要确认。

## 6. 依赖

计划的第一个任务是依赖复查（`node scripts/check-deps.mjs`）。不引入新依赖。

## 7. 错误处理

- DDL 和生成 SQL 的接口出错时，预览窗口或全局提示中显示错误文字。
- 数据标签：
  - 打开会话失败、过滤条件写错时，显示在结果面板（沿用查询标签的显示方式）；
  - 对象已被删除时，显示数据库的报错。

## 8. 测试

- **dialect**（单元测试）：
  - PG 系 DDL 的拼装：列、约束、索引、注释、视图、序列；
  - 关联查询：单列 / 多列外键、两个方向、同表多外键；
  - 数据 SQL：过滤、排序、计数、标识符引号。
- **服务层和接口**：
  - 假驱动测试接口的参数和错误；
  - DDL 在 5 个实库上的实库测试。在测试库中建带主键、唯一、外键、检查、索引、注释的表，以及视图、序列、函数，测完删除。
- **前端**（Vitest）：
  - 标签的保存和恢复（含数据标签）、同一对象只开一个；
  - 排序切换；
  - 生成 SQL 子菜单的组装（方向、置灰）。
- **浏览器实测**：5 个库，中英文界面各一遍。

## 9. 交付

- `docs/notes/m1c2-object-actions.md` 说明文档；
- 接口说明 `docs/notes/m0-4b-api.md` 补充 DDL、生成 SQL、数据 SQL 三个接口；
- M1 设计文档的进度改为"M1c2 已完成"。
