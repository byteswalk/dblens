# M1b1 SQL 编辑与执行

依据：
- 设计文档：`docs/superpowers/specs/2026-10-25-m1-workbench-design.md`
- 设计稿：`docs/design/m1-workbench.html`，第 2、3 部分

M1b 拆成两份：
- M1b1（本文）：编辑与执行；
- M1b2：智能补全与语法检查，包括 dt-sql-parser，以及 Oracle / 达梦 ANTLR 语法的技术验证。

## 结构

```text
crates/dblens-dialect/src/session.rs   执行计划语句、读取和切换当前 schema 的语句
crates/dblens-service/src/session.rs   ExecRequest：光标所在语句、出错继续、执行计划；当前 schema
crates/dblens-api/src/ws.rs            exec 消息的新字段；偏移在 UTF-16 与字节之间换算
各驱动                                  SQL 错误的出错位置（见下）
web/src/
  shared/exec.ts                 协议：新字段、确认信息记在整个状态上
  features/editor/
    tabs.ts                      查询标签（存进设置 ui.tabs）
    runner.ts                    每个标签一个执行器：会话、WebSocket、偏移换算、续取、行数上限
    dialect.ts                   方言 → Monaco 语言、sql-formatter 方言
    markers.ts · commands.ts · cursor.ts · actions.tsx
    monaco.ts · plsql.ts         本地加载 Monaco；PL/SQL 高亮（以 Monaco 的 sql 规则为基础）
    Workspace.tsx · QueryView.tsx · Toolbar.tsx · SqlEditor.tsx · ConfirmDialog.tsx
  features/results/
    results.ts · cell.ts         结果标签、消息、单元格查看的纯函数
    ResultsPanel.tsx · ResultGrid.tsx（Glide Data Grid）· CellViewer.tsx
```

## 选型复核（设计文档 2.1 节要求实现前确认）

2026-10-25 查 npm 仓库：

| 库 | 版本 | 维护情况 |
|---|---|---|
| `monaco-editor` | 0.57.0 | 2026-09 发版 |
| `@monaco-editor/react` | 4.7.0 | 最新版；用 `loader.config({ monaco })` 改为本地加载 |
| `sql-formatter` | 15.9.0 | 2026-09 发版 |
| `@glideapps/glide-data-grid` | 6.0.3 | 正式版较旧，6.0.4 的 alpha 最近一次是 2025-10，最近一年没有正式发版 |

Glide 的 peer 依赖只声明到 React 18，pnpm 安装时提示 peer 不匹配。在 React 19 上实测可用：显示、续取、双击查看都正常。如果以后出现兼容问题，备选是 AG Grid 社区版（MIT）。

## 协议的变化

接口说明见 `docs/notes/m0-4b-api.md`。WebSocket 的 `exec` 消息新增三个可选字段：
- `continue_on_error`：某条语句失败或被策略拒绝后，继续执行后面的。需要确认、被取消、连接断开时仍然停下。
- `cursor`：UTF-16 偏移，只执行它所在的那条语句。
  - 落在两条语句之间时取前一条；在第一条之前时取第一条。
  - 事件里的偏移仍是相对于整段提交的 SQL。
- `explain`：只生成执行计划，只能针对一条语句（多条时报 `invalid`）。语句按方言生成：
  - MySQL 系、PG 系：`EXPLAIN …`；
  - 达梦：`EXPLAIN FOR …`，返回计划表的各行。`EXPLAIN …` 经 DPI 执行不返回结果集（实测）。
  - Oracle：先 `EXPLAIN PLAN FOR …`，再 `SELECT … FROM TABLE(DBMS_XPLAN.DISPLAY())`。
  - 这些语句都按 `EXPLAIN` 分类，不会触发生产确认。

`confirmed` 的含义改为只对第一条语句有效：确认后从那条重新提交，后面需要确认的语句仍会停下，每条单独确认。

事件的变化：
- `statement_start` 多了 `total`，表示这次提交共几条。
  - 前面语句的结果没取完，后面的语句就执行不了。
  - 所以前端对非最后一条语句自动续取到底（受行数上限约束）；最后一条等表格滚动到底部再续取。
- `failed` 多了可选的 `offset`：数据库报告的出错位置，换算成 UTF-16 偏移。
- `confirm_required` 多了 `end`，确认框据此显示整条语句。

会话：
- 会话信息多了 `schema`，即当前 schema（MySQL 系为当前库）。
  - 打开会话时读取。
  - 执行 `USE` / `SET` / `ALTER` 开头的语句后重新读取。
- 新增 `POST /api/sessions/{id}/schema`，请求体 `{"schema": "…"}`，用于切换。

## 出错位置

`Error::Sql::position` 是语句内的字符偏移，从 1 开始。各库的来源都实测过（`SELECT '数洞', FROM t` 在 5 个库上都指向 `FROM`）：

| 库 | 来源 |
|---|---|
| PostgreSQL 系 | 协议直接给字符偏移 |
| Oracle | ODPI 给的是字节偏移，在执行处用 `Error::position_bytes_to_chars` 换成字符 |
| MySQL 系 | 错误信息 `… near 'FROM t' at line 1`：在该行起找 near 后面原文的第一行 |
| 达梦 | 错误信息 `第 1 行, 第 17 列[FROM]附近出现错误`：列号在第一行和其他行之间差 1，不可靠，所以在该行里找方括号中的词，取离列号最近的一处 |

没有位置时（如达梦的"无效的列名"、语义错误），编辑器标出整条语句。

## 前端要点

- **Monaco**：从本地打包加载（`loader.config({ monaco })`），只用编辑器自己的 worker，离线可用。工作区懒加载。
  - 每个标签一个模型（以标签 id 为路径），切换标签时保留撤销历史；关闭标签时删除模型。
  - 快捷键注册为编辑器动作；焦点不在编辑器时由外壳的全局快捷键处理。F5 在浏览器里也被拦下，不会刷新页面。
- **执行当前语句**：有选中时执行选中部分，否则把全文和光标位置交给后端，由后端按方言拆分选出那条，前端不自己拆分。
- **偏移**：执行选中部分、确认后续跑时提交的只是全文的一段。执行器收到事件后加上这段的起点，状态里存的都是全文中的偏移。
  - 执行时保存当时的全文：错误标记和失败语句的显示都按它取文本，之后的编辑不影响。
- **行数上限**：设置项 `ui.exec.row-cap`，默认 10 万行，可在设置对话框中修改。
  - 达到上限时取消这条语句（服务端在等 `fetch_more`，不取消的话会话一直忙），后面的语句也不再执行，消息里说明。
- **确认框**：取消后脚本已停下，消息中写明这条及后面的语句没有执行。
- **关闭标签**：会话有未提交的事务时，询问提交、回滚或取消。
- **结果表格**：行号、数值右对齐、NULL 灰显。
  - 列宽在第一批行到达时估计。
  - 双击或回车打开单元格查看（文本 / JSON / 十六进制）。
  - 选中区域可用 Ctrl+C 复制，按制表符分隔。
- **主题**：Monaco 的两套配色和表格的颜色都取自外壳的 CSS 变量，随主题切换。

## 实测（Windows 11；浏览器模式 + 桌面版；5 个测试库）

- **后端**：工作台实库测试 `workbench_flow` 在 5 个库上通过，覆盖读取和切换 schema、执行计划返回行、出错位置指向 `FROM`。
- **浏览器模式**：
  - PG：
    - 3 条语句的脚本按"出错停止"执行：第一条 3000 行的结果自动取完；第二条语法错误，编辑器在 `from` 下标红，后面的没有执行。
    - 改成"出错继续"后，第三条（表不存在）也执行并标红。
    - 手动提交模式：插入后工具栏和状态栏提示有未提交的事务，回滚后数据不在。
    - Ctrl+Enter 只执行光标所在的语句，光标在空行上时取前一条。
    - 执行计划、切换 schema 正常；在编辑器中执行 `SET search_path` 后下拉框随之变化。
    - 停止长查询（`pg_sleep`）后显示"已取消"，后面的语句没有执行。
    - 关闭有未提交事务的标签时询问，选回滚后会话关闭。
  - MySQL（生产连接）：
    - 标签整块标红；`DROP` 和不带 WHERE 的 `DELETE` 各自弹确认框。
    - 选"仍然执行"后继续执行；选"取消"后脚本停下，消息中说明。
    - 行数上限设为 1000 时，查询停在 1000 行，消息中说明后面的语句没有执行。
  - Oracle：
    - PL/SQL 块的 `DBMS_OUTPUT` 显示在消息中，`q'[a;b]'` 字符串不被拆开。
    - 执行计划显示 `DBMS_XPLAN` 的输出。
  - 刷新页面后标签页和其中的 SQL 恢复。
- **桌面版**：
  - 标题栏菜单有"SQL"，标题为"查询3 · ora — DBLens 数洞"。
  - 测试环境的标签顶部黄线，生产标签标红；PL/SQL 高亮正常。
  - 执行后双击单元格打开查看对话框，状态栏显示行数和耗时。

## 遗留

- **补全和语法检查**：在 M1b2 实现。
- **大字段**：单元格查看只显示预览，设计稿上的"读取完整内容"按钮需要后端按需读取 LOB 的接口，以后加。
- **PostgreSQL 手动提交时出错**：事务进入"已终止"状态，之后的语句都报 `25P02`，要先回滚。现在只显示数据库的原始错误，可以加一句提示。
- **执行计划**：以表格显示。达梦返回的是计划表的各列，不像 Oracle 那样是排好的文本。图形化执行计划不在 M1 范围。
- **单元格显示**：日期时间值按序列化后的文本显示，如 `2026-10-26T01:48:50.662695Z`，以后按类型格式化。
- **Monaco 体积**：Monaco 整包引入，各语言的高亮规则分成小块按需加载，`editor.api` 一块约 2.7 MB。只在打开第一个查询标签时加载，不影响启动。
- **收藏**：工具栏上的"收藏"和 Ctrl+D 在 M1c 实现。
