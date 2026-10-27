# M1c2 对象操作

依据：设计文档 `docs/superpowers/specs/2026-10-26-m1c2-object-actions-design.md`。M1c 的第二份（M1c1 历史与收藏已完成，M1c3 导出与运维查询在后）。

## 结构

```text
crates/dblens-dialect/src/
  ddl.rs          DdlKind；ddl_query（MySQL 系 SHOW CREATE、Oracle / 达梦 DBMS_METADATA.GET_DDL）；
                  PG 系由结构模型拼装（table_ddl = table_create + table_extras、view_ddl、sequence_ddl）；ddl_text
  browse.rs       外键关系 relations、关联查询 join_sql、数据标签的 data_sql
  meta/mod.rs     MetaKey::Ddl（不在模板文件中，只用于缓存）
crates/dblens-service/src/
  objects.rs      MetadataService::ddl / generate / relations / data_sql
  metadata.rs     run：按方言生成查询、经元数据连接池执行、按键缓存（rows 改为调用它）
crates/dblens-api/src/
  routes.rs       …/ddl、…/generate、…/relations、…/data-sql
  ws.rs           cancel 在等 fetch_more 时丢掉剩下的行（见下）
web/src/
  features/tree/  右键菜单的纯逻辑（objectMenu.ts）、DDL 预览（DdlDialog.tsx，懒加载 Monaco）、ObjectTree 的菜单和双击
  features/data/  数据标签（DataView.tsx）、排序（sort.ts）、统计行数（count.ts）
  features/editor/ 标签增加 data（tabs.ts）、Runner.rerun、工作区按标签种类显示
  features/results/ 列头点击排序（ResultGrid、ResultsPanel 的 sort / onSort）
```

## 查看 DDL

- 表、视图、物化视图、序列、同义词：
  - MySQL 系：`SHOW CREATE TABLE / VIEW / SEQUENCE`，取名字以 `Create ` 开头的列。
  - Oracle、达梦：`DBMS_METADATA.GET_DDL`。实测它给出的表 DDL 不含索引和注释，服务层用结构模型补上（`table_extras`，不含触发器）。
  - PG 系：由结构模型拼装。
  - 返回 LOB 时只读到前 64 KB，超出时在末尾注明已截断。
- 函数、过程、包、包体：各方言都用已有的程序源码查询（`MetaKey::RoutineSource`）。这里与设计文档不同：`DBMS_METADATA` 返回 CLOB，长的包会被截断；程序源码按行读取，没有这个问题。
- 达梦的 DDL 换行是 `\r\n`，统一换成 `\n`。
- 对象不存在：PG 系报 404；由数据库给出 DDL 的报数据库的错误（422）。
- 索引的 `USING 方法` 只用于 PG 系（Oracle、达梦的 NORMAL、BITMAP 不是 USING 子句）。

## 生成 SQL

- 接口参数用 `object` + `object_kind`（table / view / materialized_view），不是设计文档中的 `table`：视图也能生成 SELECT。
- 关联查询：
  - 外键关系由单独的接口 `…/relations?table=` 给出，前端右键表时取，用来组装"关联查询"子菜单；取不到时当作没有外键。
  - 子菜单项：`→ 父表 (本表的列)`、`← 子表 (子表的列)`。"全部外键"只连接本表引用的表，只有被引用时置灰。
- 插入：沿用 M1c1 的插入通道。当前不是查询标签（没有标签或是数据标签）时用该连接新开一个。
  - 实测后改：光标后面已有内容时也另起一行（之前只处理光标前），生成的 SQL 不和后面的 SQL 连在一起。

## 数据标签

- 标签：`QueryTab.data = { schema, object, objectKind, filter?, order? }`。过滤条件和排序随标签保存、恢复；同一连接的同一对象只开一个；标题为对象名，不占"查询N"的编号。
- 读取：前端调 `data-sql` 取 SQL，经标签自己的 runner 执行（首批 500 行、滚动续取、行数上限、错误显示与查询标签一致）。
- 换过滤、排序或刷新时用 `Runner.rerun`：首批读完后服务端在等 fetch_more，执行状态一直是 running，`run` 会忽略新的提交，所以先取消、等结束再执行。
- 取消还没读完的结果（实测时发现，属于 WebSocket 协议的修正）：
  - 服务端在等 fetch_more 时语句并没有在执行，数据库层面的取消不起作用，原先 pump 会一直等下去，客户端收不到 done。查询标签达到行数上限时的取消也有同样的问题。
  - 现在 cancel 同时通知 pump：在服务端读完并丢掉剩下的行（驱动要求读完才能执行下一条），其他事件照常发出，直到 done。
  - 剩下的行很多时要在服务端读完，与客户端断开时的处理相同。
  - REST 的 `/sessions/{id}/cancel` 不经过 pump，没有这个处理；前端只用 WebSocket 取消。
- 统计行数：另开一个会话执行 `COUNT(*)`，取完即关（标签自己的会话可能还有没读完的结果）。
- 数据标签执行的 SELECT 照常记入历史；生产环境不需要确认（只读）。

## 对象树

- 程序节点的键增加 routineKind：包和包体同名，原来的键相同，查看包体的 DDL 会取成包。这是实测时发现的。

## 依赖

- 2026-10-26 复查（`node scripts/check-deps.mjs`）：共 75 个直接依赖，升级 i18next 26.5.2 → 26.5.3（发布满 7 天的最新版）。
- 未满 7 天暂不升级：@tanstack/react-query 5.105.0、antd 6.6.8、react-i18next 17.1.9、react-resizable-panels 4.15.0、simple-icons 16.36.0、minijinja 3.0.4、uuid 1.29.0。

## 验证

- 后端：fmt、clippy 通过；全部测试在 `DBLENS_LANG=en` 下对 5 个实库通过。
  - 服务层的实库测试在 5 个库中建父子表（主键、唯一、外键、检查、索引、注释）、视图、序列、函数，查看 DDL、生成 SQL、取外键关系、执行数据 SQL 后删除。
  - 接口的实库测试检查参数、状态码；WebSocket 新增"等 fetch_more 时取消"的测试。
- 前端：tsc、Vitest、打包通过。
- 浏览器实测（5 个库建测试对象，1200 行子表）：
  - PostgreSQL，中文界面：右键菜单；表的 DDL（外键 ON DELETE CASCADE、检查约束、降序索引）、在新标签中打开；生成 SQL 子菜单、按外键插入关联查询；双击打开数据标签，排序（升序、降序、不排序）、WHERE 过滤、错误的条件显示数据库报错、统计行数、滚动续取到 1000 行；重新打开页面后数据标签和排序恢复。
  - Oracle（生产环境），英文界面：数据标签（标签标红）、统计行数、排序；包体的 DDL。
  - 5 个库各种对象的 DDL 和关联查询经接口逐一核对：MySQL 系的程序、序列（没有这些对象）和包等不支持的种类返回 404 / 422 / 400；Oracle、达梦的表 DDL 补上了索引和注释。
