# M4a 模型与比对

依据：设计文档 `docs/superpowers/specs/2026-10-27-m4a-model-compare-design.md`。

## 结构

```text
crates/dblens-dialect/src/compare/
  mod.rs        CompareRenames、CompareOptions、SchemaDiff；compare_schemas（表的配对、推测改名、逐表 diff_table）
  normalize.rs  比对前整理两边：本 schema 的外键引用、改名后的表名、注释、列顺序、系统生成的名字、同义的类型和默认值、外键的默认动作
  apply.rs      apply_changes：把变更应用到表结构（写回模型）
  sync.rs       sync_rebuild_tables、sync_script：同步脚本（删外键 → 删表 → 改名 → 逐表修改或重建 → 建表 → 加外键）
crates/dblens-service/src/model/
  file.rs       model.yaml、表文件的读写（serde-saphyr）、文件名转义、指纹、先写临时文件再改名
  store.rs      ModelStore：打开的模型、表文件缓存（按修改时间和长度重读）、表的保存/改名/删除、改名记录、写回
  ops.rs        最近打开的模型、新建、反向生成任务（TaskKind::Reverse）、设计器的预览和保存
crates/dblens-service/src/compare.rs  读取两边、比对、同步预览、同步执行（TaskKind::Sync）或写回模型
crates/dblens-service/src/design.rs   执行器改为按段执行（Part：一条语句或一张表的重建），M3 与同步共用
crates/dblens-api/src/models.rs       /models、/compare
web/src/features/model/    模型面板、打开/新建/反向生成对话框
web/src/features/compare/  比对标签、同步对话框、勾选与依赖提示等纯逻辑
web/src/features/design/source.ts  设计器选外键引用的数据来源（连接或模型）
```

入口：活动栏"模型"面板；标题栏"文件 → 打开模型 / 新建模型 / 结构比对"；对象树 schema 右键"与模型比对… / 另存为模型…"；
模型面板右键"与库比对…"；设计器（模型模式）工具栏"与库比对"。

## 模型文件

- `model.yaml`：`format`、`name`、`description`、`database {variant, compat, version}`、`schemas`、`renames {schema: {tables, columns}}`；
  不认识的字段保留。`compat` 为原生时不写，`version` 不写时按最新版本。
- `tables/<schema>/<表名>.yaml`：表名、注释、列（`type` 是原生类型；可空为默认，只写 `nullable: false`）、主键、唯一约束、外键、
  检查约束、索引（升序的列只写名字，降序写成 `{name, descending: true}`）、分区。逻辑类型和列序号不写，读取时按方言推出、按顺序编号；
  触发器不写。表和每列不认识的字段读进 `extra`，保存时原样写回。
- 文件名：`/ \ : * ? " < > | %`、控制字符、结尾的点和空格写成 `%XX`；Windows 保留名在第一个字符转义。表名以文件内容为准。
- 指纹：文件内容的哈希。设计器保存、删除、写回前核对。

## 与设计文档不一致的地方

- `dblens-server` 只监听本机地址时 `local_files` 为真：除了能使用模型，导出也可以直接写到页面给出的路径（与桌面程序相同）。
- MySQL 家族不查询 `lower_case_table_names`：名字是否区分大小写由比对选项"名字不区分大小写"决定（默认区分）。
- 新建模型时不选版本（按最新版本）；从库反向生成的模型带库的版本。
- 模型的改名记录在两种情况下删去已应用的项：同步到库（源是模型）的任务成功后；写回模型时（按写回的改名）。
- 比对结果树中一张表的变更按变更列表的顺序平铺（M3 的顺序已大致按列、主键、唯一、外键、检查、索引排列），没有再按类别分组。
- 比对结果另带两边的表（`source_tables`、`target_tables`），界面中对照显示、计算依赖提示。
- 外键的默认动作：`NO ACTION`（MySQL 家族的 `RESTRICT` 与之相同）与不写视为相同。
- 同步脚本另存为：桌面程序经 `POST /compare/script` 写到"另存为"选的 `.sql` 文件（只接受 `.sql`），浏览器模式下载。
- 设计器的模型模式中"SQL 预览"子标签换成"变更"子标签（变更摘要、校验错误、冲突提示），"与库比对"在工具栏。
- 库对库同步执行前也重新比对；预览后结构有变化（指纹不同）时拒绝。实库测试中同一 schema 有其他测试并发改动，不传指纹。

## 验证

- 后端：fmt、clippy 通过；全部测试在 `DBLENS_LANG=en` 下通过（683 个）。
  - dialect：表的配对与推测改名、各选项与等价写法、各家族系统生成的名字、`apply_changes` 与 `diff_table` 互逆、同步脚本的顺序和重建（PG、MySQL 快照）。
  - service：模型目录读写（文件名转义、未知字段保留、坏文件、格式较新时只读、指纹冲突、改名记录与外键引用的更新、写回前核对指纹）；
    设计器保存；反向生成失败时不留下目录；模型对模型比对并写回；预览后目标被改时拒绝同步；执行器多段执行时失败后列出没有执行的语句。
  - api：没有 `local_files` 时禁止；编辑、比对、写回模型的完整流程；脚本只写 `.sql` 文件。
  - 实库（MySQL、MariaDB、PG、Oracle、达梦、TiDB、OceanBase、openGauss PG/A、人大金仓 Oracle/PG 模式）：反向生成 → 在模型中表改名、列改名、
    加列、换索引、加外键 → 与库比对（按记下的改名配对）→ 同步到库 → 再比对无差异；库中加列后写回模型 → 两边一致；
    同一个库中第二个 schema 的库对库比对和同步（Oracle、达梦没有第二个 schema，跳过）。达梦单独运行通过（与其他测试同时运行时偶发连接失败，与以前的记录相同）。
- 前端：tsc、Vitest（勾选的默认值、依赖提示、定义对照、采用推测的改名、比对标签和模型设计器标签的保存与恢复）、打包通过。
- 浏览器实测（openGauss PG，浏览器模式监听 127.0.0.1）：反向生成对话框（从对象树 schema 右键带上连接和 schema）、模型面板的树、
  设计器的模型模式（列改名后的变更列表、保存到文件）、比对标签（按改名配对、两边定义对照标色、交换两边）、同步到库（脚本预览、执行、再比对无差异）、
  库中加列和建表后写回模型（文件内容核对）。
- 未验证：桌面程序中的"选择文件夹"对话框和"在文件管理器中显示"；监听其他地址的浏览器模式下模型面板的提示只有接口测试覆盖。
