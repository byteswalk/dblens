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

## 错误

出错时响应体就是错误对象，按 `kind` 区分，例如 `{"kind":"not_found","what":"连接配置 x"}`。状态码与 `kind` 的对应：

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
| POST | `/profiles/test` | 测试连接，`{"profile": {…}, "password": "…"}`，返回服务器信息和识别出的方言 |

配置的字段：
- 必填：`name`、`driver`（`mysql` / `postgres` / `oracle` / `dameng`）、`host`、`port`、`user`；
- 可选：
  - `database`；
  - `options`：键值对，如 `tls`、`tls_ca`、`sid`；
  - `environment`：`none` / `dev` / `test` / `prod`；
  - `color`；
  - `connect_timeout_secs`。

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
| POST | `/profiles/{id}/refresh` | 刷新元数据缓存，`{"schema": "…" \| null}` |

### 会话

| 方法 | 路径 | 说明 |
|---|---|---|
| POST | `/sessions` | 打开会话，`{"profile_id": "…"}`，返回 201 和会话信息 |
| GET | `/sessions` | 全部会话 |
| GET | `/sessions/{id}` | `{id, profile_id, variant, autocommit, in_transaction, busy, broken, schema}`；`schema` 是当前 schema（M1b1 加入） |
| DELETE | `/sessions/{id}?commit=false` | 关闭；有未提交的修改时按 `commit` 提交或回滚 |
| POST | `/sessions/{id}/autocommit` | 切换自动提交，`{"on": bool}` |
| POST | `/sessions/{id}/commit` | 提交 |
| POST | `/sessions/{id}/rollback` | 回滚 |
| POST | `/sessions/{id}/reconnect` | 连接断开后重新连接（`broken` 为 true 时，由用户确认后调用） |
| POST | `/sessions/{id}/cancel` | 取消正在执行的语句 |
| POST | `/sessions/{id}/schema` | 切换当前 schema，`{"schema": "…"}`（M1b1 加入） |

## WebSocket：`GET /api/sessions/{id}/ws?token=…`

### 客户端发送

- `{"type":"exec","sql":"…","confirmed":false,"max_rows":500}`：执行一段 SQL，可以有多条语句。
  - `confirmed` 只对第一条语句有效：确认后从那条重新提交，后面需要确认的语句仍会停下。
  - 可选字段（M1b1 加入，见 `docs/notes/m1b1-editor.md`）：
    - `continue_on_error`：失败后继续执行；
    - `cursor`：只执行这个 UTF-16 偏移所在的语句；
    - `explain`：只生成执行计划。
- `{"type":"fetch_more","max_rows":500}`：取下一批行。
- `{"type":"cancel"}`：取消。

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
