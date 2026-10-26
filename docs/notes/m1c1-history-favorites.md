# M1c1 历史与收藏

依据：设计文档 `docs/superpowers/specs/2026-10-26-m1c1-history-favorites-design.md`。M1c 的第一份（另两份：M1c2 对象操作、M1c3 导出与运维查询）。

## 结构

```text
crates/dblens-service/src/
  store.rs        迁移到版本 3：history、favorites 两张表；Store::lock 供多步事务使用
  history.rs      历史的存储（写入并清理、分页查询、删除、清空、设置）；HistoryLog：后台线程批量写入
  favorite.rs     收藏的存储：文件夹和收藏同表，增删改、移动（防止移进自己的下级）、同级重新编号
  session.rs      每条语句执行完写一条历史（执行计划、等待确认没有执行的不记）；统计取回的行数
  service.rs      Service::with_parts 启动历史写入线程；Service::with_history 供测试指定写法
crates/dblens-api/src/routes.rs   /history、/favorites 接口；tests/library.rs
web/src/
  features/history/   历史侧栏（HistoryPanel.tsx）与纯逻辑（history.ts：日期分组、摘要）
  features/favorites/ 收藏侧栏（FavoritesPanel.tsx）、收藏对话框（FavoriteDialog.tsx、dialog.ts）、树的逻辑（tree.ts）
  features/editor/    插入通道（commands.ts 的 sendInsert / onInsert）、openSql / insertSql、Ctrl+D 与工具栏星标
  features/sql/       光标所在的语句（SqlParser.statementAt，worker 新增 statement 请求）
  app/SettingsDialog.tsx   "执行历史"一组：开关、保留条数、清空
```

## 历史

- 写入：会话的执行循环在每条语句结束（成功、失败、被策略拒绝、被取消）时调用 `HistoryLog::record`，不阻塞执行。
  - 后台线程从有界队列（1024 条）取出，每批最多 200 条，在一个事务中写入并删除超出上限的旧记录。
  - 队列满或线程退出时丢弃并记日志。
- 内容：
  - 语句原文（不含结束符）、开始时间、耗时、当前 schema（语句执行前的）、执行时的连接名、驱动、变体；
  - 行数：查询为取回的行数，增删改为影响行数。行数达到上限时前端会取消读取，这条记为"已取消"，行数是已取回的数量。
  - 错误文字写入时按默认语言生成，之后不随界面语言变化。
- 不记：执行计划；需要确认、用户在确认框中取消的语句。
- 设置（经设置接口保存，由服务端读取）：
  - `ui.history.enabled`：值为 `off` 时不写入；
  - `ui.history.limit`：保留条数，默认 10000，范围 100–1000000。
  - 设计文档写的是 `history.*`，但设置接口只允许 `ui.` 开头的键（`ui.lang` 也是服务端读取的），所以改为 `ui.history.*`。
- 搜索用 `instr(lower(sql), lower(q))`：`%`、`_` 不是通配符；只对 ASCII 字母不区分大小写。
- 前端：执行结束 300 ms 后刷新历史列表（等后台写完）。

## 收藏

- 文件夹和收藏放在一张表里，`parent_id` 指向上级文件夹，删除文件夹时级联删除。
- 移动：插入到目标文件夹的第 `position` 个位置，超出时放在末尾；原文件夹和目标文件夹都重新编号，不留空位。
- 收藏 SQL：
  - 选中了内容就收藏选中的部分，否则收藏光标所在的语句（worker 按方言分句；解析器不可用时收藏全文）。
  - 名称默认取 SQL 开头，换行和空白合成一个空格，最长 60 个字符。设计文档写的是"取第一行"，实测中第一行常常只有 `SELECT`，所以改了。
  - 文件夹默认是上次选的（设置 `ui.favorites.last-folder`）。
- 整理：可以拖动，也可以在编辑对话框中换文件夹（移到末尾）。后者是实测后加的：自动化测试无法触发浏览器原生的拖放，编辑对话框提供一条可验证、也便于键盘操作的路径。
- Ctrl+D 覆盖了 Monaco 默认的"选中下一个相同的词"。

## 实现中的决定

- 插入：光标所在行在光标前已有内容时，先换行再插入，不和原有的 SQL 连在一起。
- 历史的连接筛选列出全部连接（设计文档写的是"全部 / 当前标签的连接"）。
- 历史写入线程用标准库线程和同步通道，不依赖 tokio 运行时（`Service::open` 可能在运行时之外调用）。

## 验证

- 后端：fmt、clippy 通过；全部测试在 `DBLENS_LANG=en` 下对 5 个实库通过（361 个）。
  - 第一次全量运行时，dblens-api 的达梦实库测试偶发连接失败。单独运行和再跑两遍都通过，判断是多个测试程序同时连达梦造成的。
- 前端：tsc 通过；29 个测试文件、189 个测试通过；打包通过。
- 浏览器实测（MariaDB，中英文界面）：
  - 执行三条语句（第二条出错）后，历史侧栏按"今天"分组显示两条：出错的显示 MariaDB 的报错原文，成功的显示"2 行"；第三条因出错停止没有执行，不在历史中。
  - "只看失败"、双击插入正常。
  - Ctrl+D 带出光标所在的语句，默认名称和文件夹正确；收藏后侧栏出现，新建文件夹、右键菜单、在编辑对话框中换文件夹正常。
  - 英文界面下两个侧栏和设置的文字正确，单复数正确。
- 未自动验证：鼠标拖动整理（浏览器原生拖放无法由自动化触发）。拖放后的位置计算有单元测试，移动接口有接口测试。
