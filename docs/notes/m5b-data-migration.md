# M5b 数据迁移

依据：设计文档 `docs/superpowers/specs/2026-10-28-m5b-data-migration-design.md`。

## 结构

```text
crates/dblens-dialect/src/migrate/data.rs   TableRead（增量读取、计数、抽样、按主键取一行；PG 的精确小数参数写成 $n::text::numeric）、
                                            keyset_params、达梦的 IDENTITY_INSERT、自增的调整、序列的下一个值和推进
crates/dblens-dialect/src/migrate/script.rs ExistingAction 加 Append、Replace（不动结构）、copies_data
crates/dblens-dialect/src/migrate/mod.rs    TableOverride 加 filter、structure_only
crates/dblens-service/src/migrate/
  mod.rs     MigrationPlan 加 mode、data；开始前检查（只迁数据时表必须已有、数据选项的范围）；建迁移记录、开始、继续、未完成的列表、放弃；
             作为任务执行（错误和报告用开始时的界面语言）
  record.rs  迁移记录（方案快照、脚本、每张表的搬运参数和进度、跳过的行、调整、提示、不迁移的对象），存在 数据目录/migrations/<id>.json
  run.rs     依次执行各步：建表语句 → 迁数据（按表并行）→ 建约束语句 → 调整序列和自增 → 校验；任务状态中的进度（TaskStatus::migration）
  copy.rs    一张表的搬运：流式读、转换、分批写，停止或跳过（逐行重试），续传
  values.rs  值转换成目标列的类型；校验时的规整
  report.rs  迁移报告
crates/dblens-api/src/migrations.rs  GET /migrations、POST /migrations/{id}/resume、DELETE /migrations/{id}
web/src/features/migrate/progress.tsx  按表的进度、未完成的迁移
```

## 与设计文档不一致的地方

- 自增的调整：MySQL 系写入显式值时自动推进计数器，达梦在 `IDENTITY_INSERT` 下也会，都不执行语句；PG 系用
  `setval(pg_get_serial_sequence(…), MAX + 1, false)`（标识列和 serial 通用）；Oracle 用 `START WITH LIMIT VALUE`，不先查最大值。
- 序列的推进：Oracle 一律删掉后按原定义以新起点重建（不区分版本），与达梦相同。
- 达梦的 `IDENTITY_INSERT` 只对有自增列的表打开（没有自增列的表上打开会报错）。
- 达梦序列的不缓存是 `CACHE 0`，迁到 PG 时去掉（PG 至少为 1），属于 M5a 的转换，一并修正。
- 校验按目标的规则把空串、空二进制视为 NULL（Oracle 及兼容 Oracle 的模式会这样存），否则逐值比对误报。
- 续传位置的主键值、抽样时按主键取目标行的参数，在 PG 上是精确小数时写成 `$n::text::numeric`（驱动不能直接绑定 NUMERIC 参数，与导入相同）。
- 迁移记录先放在系统临时目录的 `dblens-migrations`，`Service::open` 时改到数据目录的 `migrations`（测试各自指定目录）。
- 映射提示在开始时按界面语言写成文字存进记录，报告用记录中的文字。
- 迁移标签保存的方案在恢复时补上后来加的项（模式、数据选项），旧方案打开不会出错。

## 验证

- 后端：fmt、clippy 通过；全部测试在 `DBLENS_LANG=en` 下通过（775 个）。
  - 方言层：增量读取（单列、复合主键、过滤条件、PG 的精确小数参数）、计数、抽样、按主键取行、自增和序列的语句；追加、清空不动结构。
  - 服务层：迁移记录的读写和未完成的列表、值转换、校验的规整（含 Oracle 的空串）、语句执行（建表失败跳过依赖、成功的记下、保存记录）、开始前检查。
  - 接口：未完成的列表、继续和放弃不存在的记录。
- 实库（串行，约 7.5 分钟）：52 个——44 条路线迁结构和数据（1000 行的表、各种类型的表、推进过的序列；行数和抽样逐值比对一致，
  目标上插入新行自增不冲突，序列接着源序列往下走），4 条续传（迁了几批后取消、继续后无重复无遗漏），4 条跳过（超长的值被跳过并记进报告，清空后导入结果相同）。
- 前端：tsc、Vitest（数据选项、同名表可选的处理方式、旧方案补默认值）、打包通过。
- 浏览器：TiDB → openGauss 迁 1000 行（按表的进度、校验"一致"）；清空父表被外键挡住时表标为失败、显示错误；未完成的迁移列出后继续、放弃。
- 未验证：桌面程序的"迁移报告"保存对话框；大数据量（百万行以上）的耗时。
