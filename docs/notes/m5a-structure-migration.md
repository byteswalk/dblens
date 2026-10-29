# M5a 结构迁移

依据：设计文档 `docs/superpowers/specs/2026-10-28-m5a-structure-migration-design.md`。

## 结构

```text
crates/dblens-dialect/src/migrate/
  mod.rs     迁移设置（选项、类型规则、覆盖）、提示、MigrationDraft；migrate()：表、列、主键、唯一约束、索引、外键、检查约束、序列的转换
  names.rs   名字的大小写（按目标库的习惯）、长度上限、重名检查
  types.rs   列类型：覆盖 → 规则 → 同家族原样 → 经逻辑类型；有损判断；Oracle 的 NUMBER(p) 用整数；自增列用整数；外键动作；序列的取值范围
  expr.rs    简单表达式（检查约束、生成列、索引表达式）与默认值的转换：去掉 PG 的类型转换和 MySQL 的字符集前缀，还原 ANY (ARRAY[…])
  script.rs  两步脚本：第一步建序列、（删表）、建表和注释；第二步索引、唯一约束、检查约束，最后外键；同名表的处理；脚本全文
crates/dblens-service/src/migrate.rs   MigrationPlan、读两端结构、不迁移的对象、预览、开始前检查、任务（TaskKind::Migrate）、逐条执行、迁移报告
crates/dblens-api/src/migrations.rs    POST /migrations/preview、/migrations
crates/dblens-service/tests/migrate_live.rs   44 条迁移路线的实库测试
web/src/features/migrate/
  plan.ts          新方案、逐列/表/检查约束/序列的覆盖、同名表的处理、提示的汇总、执行前的汇总、打开另存的方案
  MigrateView.tsx  迁移标签页
```

入口：对象树 schema 右键"迁移到…"，打开迁移标签页（不绑定连接），源预填为该 schema。

## 与设计文档不一致的地方

- 接口只有两个：`/migrations/preview`（转换结果、脚本全文、开始前检查的问题、执行策略的结论）和 `/migrations`（开始）；
  "只生成脚本"用预览中的脚本全文另存，没有单独的检查和脚本接口。
- 方案多了 `sequences`（迁移的序列，None 为全部），与 `tables` 一样限定范围。
- 目标中已有同名序列时自动跳过（提示"已有同名序列"），不要求选择处理方式。
- 名字大小写的默认值：PG 系（含金仓、openGauss 兼容 Oracle 的模式）一律小写——这些库不加引号的名字都折叠为小写。
- 同家族的不同产品（如 openGauss → PG、TiDB → MySQL）：类型原样；默认值先尝试转换（openGauss 的 `pg_systimestamp()` 等），转换不了再原样。
- 系统生成的约束名（MySQL 的主键 `PRIMARY`、Oracle 的 `SYS_C…`、达梦的 `CONS…`）不带过去，由目标库生成；
  MySQL 系为外键自动建的同名索引不单独迁（在 PG、Oracle、达梦中索引和约束同名会冲突）。
- Oracle 的 `NUMBER(p)`（小数位 0、p ≤ 19）迁到其他库时用整数类型（p ≤ 4 用 16 位、≤ 9 用 32 位、否则 64 位），
  自增列在非 Oracle 目标中一律用整数：标识列才能建，外键两边类型才一致。
- 读结构的修正：TiDB、OceanBase 的 `information_schema` 中 `CURRENT_TIMESTAMP` 默认值没有 `DEFAULT_GENERATED` 标记，
  原来被当成字符串加了引号；`mysql.sql`、`tidb.sql` 改为时间列的 `CURRENT_TIMESTAMP…` 按表达式读出。
- 方案存在迁移标签中（随标签保存，切换标签不丢）；另存方案为下载 JSON 文件，打开方案用文件选择框读入。
- 检查约束的覆盖只对有名字的约束；生成列的表达式在"默认值"一列中编辑（以 `=` 提示）。

## 验证

- 后端：fmt、clippy 通过；全部测试在 `DBLENS_LANG=en` 下通过（761 个）。
  - 方言层：PG 源迁到 MySQL、Oracle、达梦、openGauss 的快照，Oracle 源迁到 PG、MySQL 的快照（含类型规则）；
    默认值、简单表达式、nextval 的转换；覆盖与跳过；重名与超长；同名表的三种处理；外键依赖两张表。
  - 服务层：开始前检查；建表失败后跳过依赖它的语句、其余照常执行。
  - 接口：方案能解析，连接不存在时 404。
- 实库：44 条路线（每个源库迁到 MySQL、PG、Oracle、达梦，另加 Oracle → 金仓、openGauss A，MySQL → TiDB、OceanBase），
  迁完读回目标结构与草稿比对（列名、逻辑类型、可空、有无默认值、自增、注释、主键、唯一约束、索引、外键、检查约束个数）；
  串行运行（`--test-threads=1`）约 3.5 分钟。
- 前端：tsc、Vitest（方案的修改、提示汇总、同名表、打开另存的方案）、打包通过。
- 浏览器：TiDB → openGauss 迁移（ENUM 列"按文本迁移"、执行、完成后重新读取显示"目标中已有"、全部设为删除重建后 DDL 预览带 DROP）；
  刷新页面后迁移标签和方案恢复。
- 未验证：桌面程序的"另存脚本""迁移报告"保存对话框。
