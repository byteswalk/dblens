# M8b GBase 8s 的其余功能

依据：`docs/superpowers/specs/2026-10-30-m8a-gbase8s-design.md` 第 1 节。放开了表设计器、导入、文档导出、结构比对、保存为模型、迁移、规范检查，删除了 `gbase.later`。

## 实测

`live.rs`：`gbase8s_import`、`gbase8s_sql_file`、`gbase8s_design`、`gbase8s_model`（含比对和同步）。
`migrate_live.rs`：GBase 到 MySQL、PostgreSQL、Oracle，MySQL、PostgreSQL 到 GBase。
界面用真实鼠标走过：入口不再置灰、规范检查出报告、设计器打开已有表。文档导出经接口生成，内容核对过。

## 服务器的行为（都是实测）

- `ALTER TABLE … MODIFY` 会删掉这一列上的单列主键、唯一、外键和检查约束，以及**别的表引用这个主键的外键**。联合约束不受影响。设计器在 `MODIFY` 后重建本表的约束，别的表的外键读不到，预览里给警告。
- 有视图依赖时，同一会话里先 `RENAME COLUMN` 再 `MODIFY` 这一列，或连续两条 `MODIFY`，报 -201。所以增、改、删列合成一条 `ALTER TABLE`，放在改列名之前。一条 `ALTER` 里 `ADD`、`DROP`、`MODIFY` 每种只能写一次。
- `DROP TABLE` 会把依赖它的视图一起删掉，不报错。有依赖视图时拒绝重建，和 PG 一样。
- `INSERT` 没有多行 `VALUES`，导入和迁移一行一条，比其他库慢。按主键更新用 `MERGE`，来源 `SELECT` 里的参数要 `CAST`。
- `sysdefaults.default` 里数字和日期时间是“内部编码 + 空格 + 值”，字符串结尾带 NUL，读取时在 `coltype::default_expr` 还原成 SQL 表达式。之前读出来是 `wQEAAAAA 1`。
- 约束自动建的索引名以空格开头。用户建的索引被约束共用时要保留，不能按 `sysconstraints.idxname` 过滤。
- 目录里没有序列的当前值，迁移时取一次 `NEXTVAL`，源序列会前进一步。
- 检查约束的表达式读回时数字被改写（`0.00` 变成 `0.`），比对时会显示为变更。
- `JDBC` 驱动目录要在设置里登记，文件名是 `gbasedbtjdbc.jar`，下载的文件带版本号，要改名。
