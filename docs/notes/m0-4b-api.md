# DBLens 接口说明（M0-4b）

桌面模式和浏览器模式共用同一套接口，由 `dblens-api` 实现，路径都在 `/api` 下，请求和响应都是 JSON。

## 鉴权

- 每个请求都要带访问令牌：
  - 优先用请求头 `Authorization: Bearer <令牌>`；
  - 浏览器的 WebSocket 不能设请求头，所以也接受查询参数 `?token=<令牌>`。
- 令牌的来源：
  - 浏览器模式：`dblens-server` 启动时打印；
  - 桌面模式：由 Tauri 注入前端（M0-4c）。
- 浏览器发出的请求（带 `Origin` 头）必须同源，或来源在 `allowed_origins` 列表中。
  - 前端开发服务器要用 `dblens-server --allow-origin http://localhost:5173` 放行。
- 令牌缺失或错误时返回 401，响应体为 `{"kind":"auth",…}`。

## 语言（M1i 加入）

- 响应和事件中给用户看的文字（错误、确认原因、驱动提示、数据库名称）按请求的语言输出，JSON 结构不变。
- 请求的语言：查询参数 `lang`（WebSocket 用）优先，其次请求头 `Accept-Language` 中的第一个语言。
  - `zh` 开头的取中文，其余取英文。
  - 都没有时用默认语言：设置 `ui.lang`，为 `auto` 时看环境变量 `DBLENS_LANG`，再看系统语言。
- 数据库返回的报错原文不翻译。
- 修改设置 `ui.lang` 后，服务的默认语言随之改变。

## 错误

出错时响应体就是错误对象，按 `kind` 区分，例如 `{"kind":"not_found","what":"连接配置 x"}`（英文为 `connection profile x`）。状态码与 `kind` 的对应：

| 状态码 | kind | 含义 |
|---|---|---|
| 400 | `invalid` | 输入不合法 |
| 401 | `auth` | 访问令牌缺失或错误（数据库认证失败是 422） |
| 403 | `denied` | 被执行策略拒绝，或 Origin 不允许 |
| 404 | `not_found` | 对象不存在 |
| 409 | `busy` / `cancelled` | 会话正忙，或已取消 |
| 423 | `locked` | 本地密码库已锁定，需要先解锁 |
| 422 | `connect` / `auth` / `sql` / `timeout` / `driver_unavailable` | 数据库一侧的问题 |
| 500 | `internal` | 程序内部错误 |

`sql` 错误带 `code`（数据库原生错误码）、`message`、`position`。

## REST

### 驱动

| 方法 | 路径 | 说明 |
|---|---|---|
| GET | `/drivers` | 各驱动的状态：是否可用、是否需要原生库、已确认的目录、建议目录、提示 |
| PUT | `/drivers/{driver}/lib-dir` | 确认原生库目录，`{"path": "绝对路径" \| null}` |

### 密码库

| 方法 | 路径 | 说明 |
|---|---|---|
| GET | `/secrets` | `{backend: "keyring" \| "vault", vault_initialized, unlocked}` |
| POST | `/secrets/init` | 首次设置主密码，`{"master": "…"}` |
| POST | `/secrets/unlock` | 解锁，`{"master": "…"}` |
| POST | `/secrets/lock` | 加锁 |

### 连接配置

| 方法 | 路径 | 说明 |
|---|---|---|
| GET | `/profiles` | 列表 |
| POST | `/profiles` | 新建，`{"profile": {…}, "password": "…" \| null}`，返回 201 |
| GET | `/profiles/{id}` | 读取 |
| PUT | `/profiles/{id}` | 修改，请求体为配置本身 |
| DELETE | `/profiles/{id}` | 删除，返回 204 |
| PUT | `/profiles/{id}/password` | 设置或清除密码，`{"password": "…" \| null}` |
| POST | `/profiles/test` | 测试连接，`{"profile": {…}, "password": "…"}`，返回服务器信息和识别出的方言；人大金仓、openGauss 另有 `compat`（`native` / `oracle`）、`mode`（探测到的兼容模式名）、`unadapted_mode`（国产库第一批加入） |

配置的字段：
- 必填：`name`、`driver`（`mysql` / `postgres` / `oracle` / `dameng`）、`host`、`port`、`user`；
- 可选：
  - `database`；
  - `options`：键值对，如 `tls`、`tls_ca`、`sid`；
  - `environment`：`none` / `dev` / `test` / `prod`；
  - `color`；
  - `connect_timeout_secs`；
  - `product`：新建连接时选的产品（如 `kingbase`、`opengauss`、`tidb`），只用于显示，连上后以识别出的变体为准；没有时按驱动显示（国产库第一批加入）。
- 只读：`variant`，上次连接成功时识别出的数据库变体（如 `kingbase`），没连接过时为 null（M1i 加入）。
  - 由服务端在连接成功时写入，修改配置时忽略请求中的值。
  - 主机、端口或驱动变了则清空。

### 元数据

| 方法 | 路径 | 说明 |
|---|---|---|
| GET | `/profiles/{id}/schemas?filter=` | schema 列表 |
| GET | `…/schemas/{schema}/objects?kind=tables\|views\|materialized_views&filter=` | 对象树节点 |
| GET | `…/schemas/{schema}/tables?table=` | 表明细；不给 `table` 时取整个 schema |
| GET | `…/schemas/{schema}/views?kind=views\|materialized_views` | 视图及其列 |
| GET | `…/schemas/{schema}/sequences` | 序列 |
| GET | `…/schemas/{schema}/routines` | 存储过程、函数、包 |
| GET | `…/schemas/{schema}/synonyms` | 同义词 |
| GET | `…/schemas/{schema}/source?kind=view_definition\|routine_source&name=&routine_kind=` | 视图定义或程序源码 |
| GET | `…/schemas/{schema}/ddl?kind=&name=` | 对象的 DDL，`{"ddl": "…"}`（M1c2 加入）。`kind`：table / view / materialized_view / sequence / function / procedure / package / package_body / synonym；该数据库没有这种对象时 400；对象不存在时 404，由数据库给出 DDL 的（MySQL 系、Oracle、达梦）则是数据库的错误（422） |
| GET | `…/schemas/{schema}/generate?object=&object_kind=&kind=&ref_schema=&ref_table=` | 生成 SQL，`{"sql": "…"}`（M1c2 加入）。`kind`：select / insert / update / delete / join；`object_kind`：table（默认）/ view / materialized_view，视图只能 select；update、delete 需要主键；join 给出 `ref_table` 时只连接与它相关的外键，否则连接本表引用的全部表 |
| GET | `…/schemas/{schema}/relations?table=` | 表的外键关系（M1c2 加入）：`[{direction: out\|in, name, schema, table, local_columns, other_columns}]`，`in` 只含同一 schema 中引用它的表 |
| POST | `…/schemas/{schema}/data-sql` | 数据标签的 SQL（M1c2 加入），`{"object", "filter"?, "order"?: {"column", "desc"}, "count"?}` → `{"sql": "…"}`；`filter` 为空白时不加 WHERE，`count` 为 true 时生成 `SELECT COUNT(*)` |
| POST | `/profiles/{id}/refresh` | 刷新元数据缓存，`{"schema": "…" \| null}` |
| GET | `/profiles/{id}/ops` | 运维查询（M1c3）：`[{key, name, category, sql}]`，`name` 按请求的语言，`category` 为 `session` / `lock` / `storage` / `sql`。按连接上次识别出的变体选模板，没连接过时按驱动；不连接数据库 |

### 会话

| 方法 | 路径 | 说明 |
|---|---|---|
| POST | `/sessions` | 打开会话，`{"profile_id": "…"}`，返回 201 和会话信息 |
| GET | `/sessions` | 全部会话 |
| GET | `/sessions/{id}` | `{id, profile_id, variant, compat, autocommit, in_transaction, busy, broken, schema}`；`schema` 是当前 schema（M1b1 加入）；`compat` 是兼容模式（国产库第一批加入） |
| DELETE | `/sessions/{id}?commit=false` | 关闭；有未提交的修改时按 `commit` 提交或回滚 |
| POST | `/sessions/{id}/autocommit` | 切换自动提交，`{"on": bool}` |
| POST | `/sessions/{id}/commit` | 提交 |
| POST | `/sessions/{id}/rollback` | 回滚 |
| POST | `/sessions/{id}/reconnect` | 连接断开后重新连接（`broken` 为 true 时，由用户确认后调用） |
| POST | `/sessions/{id}/cancel` | 取消正在执行的语句 |
| POST | `/sessions/{id}/schema` | 切换当前 schema，`{"schema": "…"}`（M1b1 加入） |

### 历史（M1c1 加入）

每条执行过的语句由服务端在执行时写入（执行计划和等待确认而没有执行的语句不记），保留条数和开关见设置 `ui.history.limit`（默认 10000）、`ui.history.enabled`（`off` 为关闭）。

| 方法 | 路径 | 说明 |
|---|---|---|
| GET | `/history?q=&profile=&failed=true&before=&limit=` | 按时间倒序分页：`before` 为上一页最后一条的 `id`，`limit` 默认 100、最大 500；`q` 在 SQL 中做不区分大小写的包含匹配；`failed=true` 只返回有错误的 |
| DELETE | `/history/{id}` | 删除一条，返回 204 |
| DELETE | `/history` | 清空，返回 204 |

每条：`{id, at, profile_id, profile_name, driver, variant, schema, sql, elapsed_ms, rows, error}`。`at` 为 Unix 毫秒；`rows` 为查询取回的行数或增删改影响的行数；`error` 为失败、被拒绝或被取消时的错误文字（写入时按默认语言），成功时为 null。

### 收藏（M1c1 加入）

| 方法 | 路径 | 说明 |
|---|---|---|
| GET | `/favorites` | 全部文件夹和收藏，平铺，按上级和次序排列 |
| POST | `/favorites` | 新建，`{parent_id, kind: "folder" \| "item", name, sql?, note?, driver?, variant?}`，放在上级的末尾，返回 201 |
| PUT | `/favorites/{id}` | 修改，`{name?, sql?, note?, move?: {parent_id, position}}`；`note` 为空字符串时清除；移动时插入到 `position`，同一文件夹中的其余项依次后移 |
| DELETE | `/favorites/{id}` | 删除，文件夹连同其下内容；返回 204 |

每条：`{id, parent_id, kind, name, sql, note, driver, variant, position, created_at, updated_at}`。收藏必须有 SQL，文件夹没有；上级必须是文件夹；不能把文件夹移到它自己或它的下级中（都返回 `invalid`）。

### 表数据编辑（M2a 加入）

| 方法 | 路径 | 说明 |
|---|---|---|
| POST | `/profiles/{id}/changes/preview` | 预览改动集生成的语句，返回 `[{index, sql}]`（`sql` 中的值写成字面量），不执行 |
| POST | `/sessions/{id}/changes` | 在会话中执行改动集，返回 `{applied, failed}`。失败时已整体回滚，`failed` 为 `{index, error}`（仍是 200）；会话正忙时 409 |

- 改动集：`{schema, table, changes: [...]}`，每个改动按 `kind` 区分：
  - `{"kind": "update", "key": {列: 值}, "original": {列: 原值}, "values": {列: 新值}}`；
  - `{"kind": "insert", "values": {列: 值}}`；
  - `{"kind": "delete", "key": {列: 值}}`。

  值的格式同结果集（`{"t": 类型, "v": 值}`）。`key` 必须正好是表的行标识列（主键，没有时为所有列非空的唯一键）。
- 表没有行标识、引用了不存在的列、写了生成列时 400（`invalid`）。
- 执行时每条语句必须恰好影响 1 行：为 0 时报"已被修改或删除"，大于 1 时报行标识不唯一。

### 导出（M1c3 加入）

| 方法 | 路径 | 说明 |
|---|---|---|
| POST | `/exports` | 提交导出任务，返回 201 和 `{"id"}`。请求体：`{profile_id, source, format, options?, path?, file_name}` |
| GET | `/exports/{id}` | `{id, state, rows, warnings: {lob_truncated, cell_truncated}, error, file_name, path}`；`state` 为 `running` / `done` / `failed` / `cancelled`，`error` 是错误对象 |
| DELETE | `/exports/{id}` | 进行中时取消；已结束时删除任务记录和临时文件。204 |
| GET | `/exports/{id}/file?token=…` | 浏览器模式下载（`Content-Disposition` 带文件名）。下载后删除临时文件，任务不再存在 |

- `source`：`{"sql": "…"}`（必须是单条查询语句，否则 400）或 `{"data": {schema, object, filter?, order?}}`（由数据标签的 SQL 生成）。
- `format`：`csv` / `xlsx` / `insert` / `json`。`options`：`delimiter`（`,` / `	` / `;`）、`header`、`bom`、`null_text`、`table`（INSERT 的目标表）、`rows_per_insert`。
- `path`：只有桌面程序的本机服务（`ApiConfig::local_files`）接受，必须是绝对路径且所在目录存在；浏览器模式给出时 400。不给时写到数据目录的 `exports/` 下，完成后下载。
- 同时进行的任务最多 4 个，超出时 409（`busy`）。已结束的任务保留 1 小时。
- 导出另开一个连接执行，大字段每个值最多读 256 MB；照常记入执行历史。

### 导入与执行 SQL 文件（M2b 加入）

| 方法 | 路径 | 说明 |
|---|---|---|
| POST | `/uploads?name=原文件名` | 请求体是文件内容（不限于 2 MB，上限见下），边收边写到数据目录的 `imports/`。返回 201 和 `{id, name, size}` |
| POST | `/imports/preview` | `{source, file}` → `{columns, rows, types, warnings: {ragged_rows, bad_encoding, error_cells}, sheets, encoding, delimiter}`：前 100 行，`types` 按前 1000 行推断 |
| POST | `/profiles/{id}/imports/ddl` | `{schema, table, create}` → `{sql}`：新建表的建表语句 |
| POST | `/imports` | 提交导入任务，返回 201 和 `{"id"}`。请求体：`{profile_id, source, file, schema, table, create?, mapping: [{source, target}], mode, transaction?, batch_size?, on_error?, empty_as_string?}` |
| POST | `/sql-files` | 提交执行 SQL 文件的任务，返回 201 和 `{"id"}`。请求体：`{profile_id, source, encoding?, schema?, continue_on_error?, transaction?}` |
| GET | `/tasks/{id}` | 任务状态：`{id, kind, state, phase, progress, warnings, error, error_line, committed_line, failures, confirm, report, remaining}` |
| DELETE | `/tasks/{id}` | 进行中（或等待确认）时取消；已结束时删除任务和报告。204 |
| POST | `/tasks/{id}/confirm` | `{yes}`：回答确认。不在等待确认时 400 |
| GET | `/tasks/{id}/report?token=…` | 下载报告（导入的出错报告 CSV、SQL 文件的执行日志）。任务过期前可以多次下载 |
| POST | `/tasks/{id}/report` | `{path}`：桌面程序把报告复制到"另存为"选的路径；浏览器模式 400 |
| GET | `/tasks/{id}/statements/{n}` | `{sql}`：前 100 条失败中第 n 条（从 0 开始）的完整语句 |

- `source`：`{"upload": id}`（上传的文件），或 `{"path": "…"}`（本机路径，只有桌面程序接受，必须是存在的文件）。
- `file`：`{format: "csv" | "excel", encoding?: "auto" | "utf8" | "gbk", delimiter?, header?: true, sheet?}`。
- `create`：`{columns: [{name, logical, nullable?}], primary_key: [列名]}`；`logical` 为逻辑类型的 JSON（如 `{"kind": "var_char", "len": 50}`）。新建表时 `mode` 只能是 `append`。
- `mode`：`append` / `replace`（清空后导入，固定为整体一个事务）/ `upsert`（按主键更新，要求主键列都已映射）。`transaction`：`whole` / `batch`（默认，`batch_size` 默认 1000，范围 100–100000）；`on_error`：`stop`（默认）/ `skip`。
- 任务的 `state`：`running` / `confirm`（等待确认，`confirm` 为 `{items: [{line, text, reason}], more}`）/ `done` / `failed` / `cancelled`。`phase`：`preparing` / `scanning` / `creating` / `clearing` / `writing` / `executing` / `finished`。
- `progress`：`{read, total, rows_read, rows_written, rows_skipped, statements_done, statements_total, statements_failed, affected}`；`read`/`total` 为已读字节和文件大小（Excel 为行数）。
- `failures`：至多 100 条 `{index, line, error, text}`。导入为跳过的行（`line` 为文件行号），SQL 文件为失败的语句（`index` 为语句序号）。
- 上传大小上限：设置项 `ui.import.upload_limit_mb`（MB），默认 2048。超过时 400，已收到的部分删除。
- 同时进行的任务最多 4 个，超过时 409。

### 表设计器（M3 加入）

| 方法 | 路径 | 说明 |
|---|---|---|
| GET | `/profiles/{id}/design/caps` | 方言能力和类型目录：`{reorder, checks, descending_index, on_delete, on_update, index_methods, types: [{name, params, char_unit}]}`；`params` 为 `none` / `length` / `precision_scale` |
| GET | `/profiles/{id}/design/table?schema=&name=` | 原表（直接从库读取，不走缓存）：`{table, storage: {engine, charset, collation, tablespace, row_count}, fingerprint}` |
| POST | `/profiles/{id}/design/preview` | 预览，不执行：`{errors, changes, statements, rebuild: {reasons, active, refused}, warnings, transactional, denied, confirm, conflict}` |
| POST | `/profiles/{id}/design/apply` | 执行，返回 201 和 `{"id"}`（任务，`kind` 为 `design`，状态用 `/tasks/{id}`）。有校验错误、原表已被修改、不能重建、没有改动时 400，被执行策略拒绝时 403 |

- 请求体：`{schema, name, draft, renames: {columns, indexes}, rebuild, fingerprint}`。`name` 为已有表的名字，新建表时为 null；`draft` 是完整的表结构（与 `/schemas/{schema}/tables` 返回的相同）；`renames` 为"新名 → 原名"；`rebuild` 为用户手动选择重建；`fingerprint` 为打开时读到的指纹，执行前核对。
- `errors`：`[{section, row, field, message}]`，`section` 为 `table` / `columns` / `primary_key` / `indexes` / `foreign_keys` / `uniques` / `checks`，`row` 是该部分中的序号（不含标为删除的行）。
- `changes`：变更列表，每项 `{kind, …}`，`kind` 为 `create_table`、`set_table_comment`、`add_column`、`drop_column`、`rename_column`、`alter_column`、`move_column`、`add_*` / `drop_*`（`primary_key`、`unique`、`check`、`foreign_key`、`index`）、`rename_index`。
- `statements`：`[{kind: "sql", sql, changes} | {kind: "count_check", source, target, changes}]`；`count_check` 是重建时由程序执行的核对行数。
- 任务失败时 `remaining` 为没有执行的语句（以 `;` 分隔），PG 家族整体回滚时为 null。

### 模型与比对（M4a 加入）

模型目录是服务所在机器上的路径：`/models` 下的接口和 `/compare/script` 只在页面和服务在同一台机器上（桌面程序、只监听本机地址的 `dblens-server`）时可用，否则 403。

| 方法 | 路径 | 说明 |
|---|---|---|
| GET | `/models` | `{open: [模型], recent: [目录]}`；模型为 `{id, path, info, read_only}`，`info` 是 `model.yaml` 的内容 |
| POST | `/models/open` | `{path}`，打开模型目录；已经打开的返回原来的编号 |
| POST | `/models` | `{path, name, database: {variant, compat?, version?}}`，新建空模型（目录必须不存在或为空），201 |
| POST | `/models/reverse` | `{profile_id, schemas, path, name}`，反向生成，返回 201 和 `{"id"}`（任务，`kind` 为 `reverse`；结束后 `model` 为打开的模型的编号） |
| GET / PUT / DELETE | `/models/{id}` | 重新加载并返回模型 / 改 `{name, description, schemas}` / 关闭 |
| GET | `/models/{id}/caps` | 设计器的能力和类型目录（按模型的数据库类型），格式同 `/profiles/{id}/design/caps` |
| GET | `/models/{id}/tables` | `[{schema, name, comment?, error?: {path, message}}]`；`error` 是解析失败的文件 |
| GET / DELETE | `/models/{id}/tables/{schema}/{name}` | `{table, fingerprint}` / 删除表文件（`?fingerprint=` 时核对） |
| POST | `/models/{id}/tables/{schema}/{name}/rename` | `{to}`，表改名：改文件名、更新其他表外键的引用、记入改名记录 |
| POST | `/models/{id}/tables/preview` | 设计器的检查：`{errors, warnings, changes, conflict}` |
| POST | `/models/{id}/tables` | 保存设计器中的表，返回 `{fingerprint}`；有校验错误、文件已被修改时 400 |
| POST | `/compare` | 比对：`{source, target, options, renames}` → `{diff: {tables, suggestions}, renames, source_errors, target_errors, source_tables, target_tables, fingerprint}` |
| POST | `/compare/preview` | 同步的预览：`{compare, selected, fingerprint}` → `{script, text, writes, denied, confirm, fingerprint}` |
| POST | `/compare/sync` | 同步：目标是库时 201 和 `{task}`（任务，`kind` 为 `sync`），目标是模型时 200 和 `{task: null}`（已写回）。比对结果与预览时不同时 400 |
| POST | `/compare/script` | `{path, text}`，把同步脚本写到 `.sql` 文件（桌面程序的"另存为"） |

- 设计器请求体：`{schema, name, draft, renames, fingerprint}`，含义同 `/profiles/{id}/design/preview`（没有 `rebuild`）。
- 比对的一边：`{kind: "model", model, schema}` 或 `{kind: "database", profile, schema}`；结果描述"怎样修改目标才与源一致"。
- `options`：`{ignore_comments, ignore_column_order, ignore_generated_names, case_insensitive}`，`ignore_generated_names` 默认为 true。
- `renames`：采用的改名，`{tables: {源: 目标}, columns: {源表: {源列: 目标列}}}`；模型中记下的改名自动采用。
- `diff.tables`：`[{status, source, target, items: [{id, change}], appended}]`，`status` 为 `added` / `removed` / `renamed` / `changed` / `same`；
  `change` 为 `{kind: "drop_table", name}`、`{kind: "rename_table", from, to}` 或 `{kind: "table", change: 表设计器的变更}`。勾选和同步按 `id`。
- `diff.suggestions`：推测的改名 `[{table, source, target}]`，`table` 为 null 时是表的改名。
- `script`（目标是库）：`{steps: [{kind: "sql", sql, items} | {kind: "rebuild", table, rebuild, items}], refused: [[表, 原因]], warnings, transactional}`；
  `writes`（目标是模型）：`[{name, from?, delete}]`。

## WebSocket：`GET /api/sessions/{id}/ws?token=…&lang=…`

`lang` 可选，决定事件中文字的语言（见"语言"一节）。

### 客户端发送

- `{"type":"exec","sql":"…","confirmed":false,"max_rows":500}`：执行一段 SQL，可以有多条语句。
  - `confirmed` 只对第一条语句有效：确认后从那条重新提交，后面需要确认的语句仍会停下。
  - 可选字段（M1b1 加入，见 `docs/notes/m1b1-editor.md`）：
    - `continue_on_error`：失败后继续执行；
    - `cursor`：只执行这个 UTF-16 偏移所在的语句；
    - `explain`：只生成执行计划。
- `{"type":"fetch_more","max_rows":500}`：取下一批行。
- `{"type":"cancel"}`：取消。服务端正在等 `fetch_more` 时（语句并没有在执行），丢掉还没读的行，接着发后面的事件直到 `done`（M1c2 加入）。

### 服务端发送

| type | 字段 | 说明 |
|---|---|---|
| `statement_start` | `index, total, start, end, info` | 开始执行第 `index` 条语句，这次提交共 `total` 条。`start`/`end` 是它在提交的 SQL 中的 UTF-16 偏移（可直接用于 Monaco）。`info` 是语句分析结果 |
| `result_start` | `columns` | 结果集的列 |
| `rows` | `rows` | 一批行，每个值形如 `{"t": 类型, "v": 值}` |
| `result_end` | — | 结果集结束 |
| `affected` | `count` | 影响的行数 |
| `message` | `text` | 服务器消息（PG notice、DBMS_OUTPUT） |
| `statement_end` | `index, elapsed_ms` | 这条语句结束 |
| `confirm_required` | `index, start, end, reason` | 这条语句需要确认，脚本停在这里。确认后从 `start`（UTF-16 偏移）处截取剩余 SQL，带 `confirmed: true` 重新发送 |
| `failed` | `index, error, offset?` | 这条语句失败、被拒绝或被取消，脚本停在这里（请求了 `continue_on_error` 时继续）。`offset` 是出错位置的 UTF-16 偏移，数据库没报告时省略 |
| `done` | — | 整段 SQL 结束 |
| `error` | `error` | 协议层面的错误，如消息无法解析、上一段还没执行完 |

### 背压与断开

- 服务端每发出一个装满 `max_rows` 行的 `rows`，就停下等 `fetch_more`。
- 一批不满，说明结果集已取完，服务端会直接发后面的事件。
- WebSocket 断开时，正在执行的语句会被取消，会话恢复空闲。
