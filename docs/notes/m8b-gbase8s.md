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

## 桌面窗口验证（dev 构建）

用 `dblens-desktop` 的 debug 构建加 `DBLENS_DATA_DIR`，系统级真实鼠标走过：MySQL 生产连接的建用户、授权、回收、锁定、改密、输入主体名删除；GBase 的权限只读、谁能访问、设计器、规范检查、文档导出对话框。

这一轮查出的问题：

- 规范检查没有拆开带库的 schema，SQL Server 和 GBase 从树上发起时读到空结构，报告通过率 100%。修复后 GBase 的结果是 58.6%、36 个问题。
- 桌面程序随机到 WebView 拒绝的端口（如 5061）时窗口打不开（ERR_UNSAFE_PORT），偶发。绑定时避开这些端口。
- 文档导出的 Schema 和默认标题露出分隔符，规范检查的标题同样；状态栏在规范检查标签上写成“比对”。

第二轮桌面验证：

- `capabilities/main.json` 漏了 `open_dialog`、`folder_dialog`，桌面版的“选择文件”“选择文件夹”点了没反应，只在日志里有 `not allowed`。已放行，并加了测试：`generate_handler!` 里的命令都要在权限清单和 `build.rs` 里。
- 改被引用的主键或唯一列时，设计器现在在脚本最后重建别的表的外键，不再只给警告（`design_restore_keys`）。类型改变时重建可能失败，预览里有提示。
- 17 个实例在桌面程序里都连上并展开过：MySQL、MariaDB、TiDB、OceanBase、PostgreSQL、openGauss（PG、A、SHA 账号）、金仓（两种模式）、GaussDB（两种模式）、Oracle、达梦、SQL Server 2022、2025、GBase 8s。
- TiDB 的系统库名是大写且比较区分大小写，没有归进“系统”，改成 `LOWER()` 比较，并加了 `metrics_schema`、`oceanbase`。
- GaussDB 开发者版读不到角色成员关系，权限页报告范围不完整。GaussDB 不单独适配，未处理。
