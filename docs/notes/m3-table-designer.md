# M3 表设计器

依据：设计文档 `docs/superpowers/specs/2026-10-27-m3-table-designer-design.md`。

## 结构

```text
crates/dblens-dialect/src/design/
  mod.rs       Renames、TableChange、Diff、DesignIssue；Dialect::prepare_draft（重新解析逻辑类型、主键列 NOT NULL）
  diff.rs      validate（校验草稿）、diff_table（比对出变更列表）
  catalog.rs   design_caps：类型目录和方言能力
  sql.rs       design_unsupported、design_sql（按家族分派）、design_transactional；共用的外键、检查约束、索引列写法
  mysql.rs / postgres.rs / oracle.rs（含达梦）   各家族的 DDL
  rebuild.rs   重建表的脚本（RebuildContext → RebuildOutcome）
crates/dblens-dialect/sql/*.sql   新增 referencing_keys、dependent_views、grants、table_storage、trigger_ddl（只有 Oracle）
crates/dblens-service/src/design.rs   design_caps / design_table / preview_design / start_design；执行任务（Run）
crates/dblens-api/src/design.rs       /profiles/{id}/design/{caps,table,preview,apply}
web/src/features/design/
  logic.ts          草稿（每行 key、origin、deleted）、改名跟踪、类型的拆分与拼接、改动标记、校验错误换回草稿中的行
  designStore.ts    每个设计器标签的状态
  summary.ts        变更摘要、脚本文本
  DesignView.tsx    标签页：工具栏、子标签、自动预览、执行进度
  ColumnsPane / IndexesPane / ForeignKeysPane / ConstraintsPane / OptionsPane / ListPane / SaveDialog / SqlBox
```

入口：对象树表的右键"设计表"、schema 的右键"新建表"；数据标签工具栏"设计表"；标题栏"文件 → 新建表"。

## 与设计文档不一致的地方

- `Renames` 只有列和索引：约束只改名也按先删后建处理，不需要映射。
- 库不支持的改动不放在 `Diff` 里，由 `Dialect::design_unsupported(&Diff)` 判断；`Diff` 另有 `appended`（新列都在最后、保留的列相对顺序不变），不能指定列位置的库据此判断是否要重建。
- MySQL 家族每项改动一条 `ALTER TABLE`，不合成一条：同一条 ALTER 中改名与 MODIFY、AFTER 引用的是改名前还是改名后的列，各版本行为不一；MySQL 8 的多数列操作是 INSTANT / INPLACE，分开执行的代价不大。例外：换主键时删除和新增放在同一条（自增列必须是键，单独删主键报 1075）；既修改又挪动的列只在挪动时 MODIFY 一次。
- 一条生成的语句对应的变更序号是列表（`changes`）。
- 变更的可读摘要由前端按 `changes` 生成（随界面语言），接口不返回文字摘要。
- 没有"只读连接"这个概念：预览中只标出被执行策略拒绝的语句（`denied`）和需要确认的原因（`confirm`）；生产环境的横幅由前端按连接的环境显示。
- 不能指定列位置的库，"新增列"加在最后（插在中间就要重建表）；仍可以挪动，挪动后保存时重建。
- 索引方法不写与默认方法（BTREE、NORMAL）视为相同（实库中读回的索引都带方法）。
- Oracle 的 `triggers` 只有触发器体，重建时另用 `trigger_ddl` 由 `ALL_TRIGGERS.DESCRIPTION` 拼出完整的 CREATE TRIGGER；执行前把当前 schema 切到表所在的 schema（MySQL、Oracle 的触发器 DDL 中表名不带 schema）。
- 达梦的 `ALL_TAB_PRIVS` 用 `OWNER` 列（Oracle 是 `TABLE_SCHEMA`）。
- 重建：MySQL 的新表带主键（自增列必须是键），其他库在改回原名之后再建主键（主键索引名在 schema 内不能重复）；表和列的注释建在临时表上，改名后保留。
- 改动标记按行显示（新增绿、修改黄、删除加删除线），没有逐个单元格标黄；选中行的原生类型在属性面板中改过时标色。
- 标题栏"文件 → 新建表"的 schema：当前是数据标签或设计器标签时用它的 schema，否则在"选项"中选。
- 执行的每条语句各记一条执行历史。

## 验证

- 后端：fmt、clippy 通过；全部测试在 `DBLENS_LANG=en` 下通过。
  - dialect：比对、校验、各家族每种变更的快照、版本差异（MySQL 5.6 的 CHANGE、MariaDB 的 DROP CONSTRAINT）、生成的每条语句经拆分仍是一条、重建脚本快照（四个家族）、拒绝重建、临时表名截断。
  - service（假驱动）：新建表的预览和执行（一个事务、执行历史）；重建在删除旧表之前失败时删除新表、之后失败时列出没有执行的语句。
  - 实库（5 个库）：用设计器新建表；改名并加长、改可空/默认值/注释、换索引、加列后读回与草稿比对无差异；原表被别人改过时拒绝执行；强制重建（调整列顺序、VARCHAR 改为数值类型）后数据、约束、索引、注释、触发器、其他表指向本表的外键、自增接续、授权（PG、Oracle、达梦授予 PUBLIC）、依赖视图（MySQL 按名字、Oracle 和达梦重新编译；PG 有依赖视图时拒绝）都正确；中途失败时 PG 全部回滚、其他库保留已执行的。
  - 达梦的测试与其他测试同时连库时偶发连接失败，单独串行运行通过（与以前的记录相同）。
- 前端：tsc、Vitest、打包通过。
- 浏览器实测（中文、英文界面）：PG 上从 schema 右键新建表（类型下拉与长度、小数位的启用规则，主键、自增、注释），保存后标签转为编辑这张表；改名加挪动列触发重建（确认框中重建开关强制打开），执行后列顺序正确；未保存时关闭标签的提示；数据标签的"设计表"按钮切到已打开的设计器；MySQL 上新建带降序索引和检查约束的表并读回核对；Oracle 生产环境连接的确认框横幅和红色执行按钮；"在新查询标签中打开"。
- 未验证：重建执行中途的取消只有单元测试覆盖（与失败走同一条清理路径）；桌面程序中的设计器没有单独实测。
