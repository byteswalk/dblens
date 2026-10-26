# M1c1 历史与收藏 设计

日期：2026-10-26。上级设计：`docs/superpowers/specs/2026-10-25-m1-workbench-design.md`（1 节侧栏、3 节后端新增、4 节拆分）。
M1c 拆成三份（2026-10-26 与用户确认），按顺序执行：
- **M1c1 历史与收藏**（本文）；
- M1c2 对象操作：查看 DDL、生成 SQL、关联查询、"查看数据"标签；
- M1c3 导出与运维查询。

## 1. 目标与范围

- **历史侧栏**：记录每条执行过的语句，可搜索、按连接或失败过滤，可插入编辑器或在新标签中打开。
- **收藏侧栏**：多级文件夹；Ctrl+D 或工具栏星标收藏当前 SQL；可插入、打开、整理。
- **设置**："历史"一组。

不做：
- 历史的导入导出；
- 收藏的导入导出和团队共享（以后做团队服务器时再设计）；
- 审计日志（与历史分开设计）。

## 2. 已定的决策（2026-10-26 与用户确认）

| 问题 | 结论 |
|---|---|
| 历史保存 | 全部记录（含生产连接），默认保留最近 1 万条；设置可调条数、可关闭、可清空 |
| 收藏组织 | 多级文件夹；收藏不绑定连接，记下保存时的数据库类型作提示 |
| 历史由谁写 | 后端在执行时写入（会话每执行完一条语句写一条），不依赖前端 |
| 双击行为 | 插入到当前编辑器的光标处；"在新标签中打开"放在右键菜单 |
| "只看失败" | 包括被取消的语句 |

## 3. 数据（本地 SQLite，迁移到版本 3）

```sql
CREATE TABLE history (
  id INTEGER PRIMARY KEY,
  at INTEGER NOT NULL,            -- 执行开始时间，Unix 毫秒
  profile_id TEXT NOT NULL,
  profile_name TEXT NOT NULL,     -- 执行时的连接名（连接删除后仍能显示）
  driver TEXT NOT NULL,
  variant TEXT,                   -- 识别出的数据库变体（图标用）
  schema TEXT,                    -- 执行时的当前 schema
  sql TEXT NOT NULL,              -- 这一条语句的原文（不含结束符）
  elapsed_ms INTEGER,
  rows INTEGER,                   -- 查询取回的行数，或增删改影响的行数；不知道时为空
  error TEXT                      -- 失败或被取消时的错误文字（按默认语言渲染）；成功时为空
);
CREATE INDEX history_at ON history(at);
CREATE INDEX history_profile ON history(profile_id, at);

CREATE TABLE favorites (
  id TEXT PRIMARY KEY,            -- uuid
  parent_id TEXT REFERENCES favorites(id) ON DELETE CASCADE,  -- 上级文件夹；空为根
  kind TEXT NOT NULL,             -- 'folder' | 'item'
  name TEXT NOT NULL,
  sql TEXT,                       -- 收藏的 SQL（文件夹为空）
  note TEXT,                      -- 说明
  driver TEXT,                    -- 保存时的驱动（图标用）
  variant TEXT,
  position INTEGER NOT NULL,      -- 同一文件夹内的次序
  created_at INTEGER NOT NULL,
  updated_at INTEGER NOT NULL
);
CREATE INDEX favorites_parent ON favorites(parent_id, position);
```

- 删除文件夹时连同其下的内容一起删除（`ON DELETE CASCADE`，连接打开时启用 `PRAGMA foreign_keys`）。
- 移动收藏时不能移进自己或自己的下级：后端检查，违反时返回 `invalid`。

## 4. 历史的写入

- **时机**：会话执行脚本时，每条语句结束写一条：
  - 正常结束（`StatementEnd`）；
  - 失败、被拒绝或被取消（`Failed`）。
  - 需要确认而没有执行的语句（`ConfirmRequired` 后用户取消）不记。
  - 执行计划（`explain`）不记。
- **内容**：
  - 行数：查询为本条语句各结果集已取回的行数之和（达到上限停止读取时，为已取回的数量），增删改为影响行数。
  - schema：语句开始时会话的当前 schema。
- **写入方式**：
  - 不阻塞执行：会话把记录发到服务中的写入任务（有界通道，满了丢弃并记日志），由写入任务批量写库。
  - 每次写入后，删除超出上限的最旧记录。
- **设置**（存放在 settings 表，由后端读取）：
  - `history.enabled`：值为 `off` 时关闭，此时不写入；已有记录保留。
  - `history.limit`：保留条数，默认 10000，允许 100 到 1000000；改小时下次写入就会删掉超出的部分。

## 5. 接口

| 方法 | 路径 | 说明 |
|---|---|---|
| GET | `/api/history?q=&profile=&failed=1&before=&limit=` | 倒序分页（`before` 为上一页最后一条的 id，`limit` 默认 100、最大 500）。`q` 在 SQL 中做不区分大小写的包含匹配，`failed=1` 只返回有错误的 |
| DELETE | `/api/history/{id}` | 删除一条 |
| DELETE | `/api/history` | 清空 |
| GET | `/api/favorites` | 全部文件夹和收藏，平铺返回，前端组成树 |
| POST | `/api/favorites` | 新建，请求体 `{ parent_id, kind, name, sql?, note?, driver?, variant? }`，放在该文件夹的末尾 |
| PUT | `/api/favorites/{id}` | 修改 `{ name?, sql?, note?, parent_id?, position? }`，移动时按 `position` 插入，同文件夹其余项依次后移 |
| DELETE | `/api/favorites/{id}` | 删除（文件夹连同其下内容） |

- 历史中的 `error` 写入时按默认语言渲染成文字，之后不随界面语言变化；数据库返回的报错原文本来就不翻译。
- 错误沿用现有的 `Error` 类别和中英文词条。

## 6. 界面

**历史侧栏**
- **顶部**：搜索框（输入后 300 ms 查询）、连接筛选（全部 / 当前标签的连接）、"只看失败"开关。
- **列表**：
  - 按"今天 / 昨天 / 日期"分组；
  - 每条显示 SQL 第一行、连接名（带环境色点和数据库图标）、时间、耗时和行数，失败的以红色显示错误摘要；
  - 滚动到底部时加载下一页；
  - 任一标签页执行结束后自动刷新。
- **操作**：
  - 悬停：预览完整 SQL；
  - 双击：插入到当前编辑器的光标处，没有打开的标签时用该条的连接新开一个；
  - 右键：在新标签中打开（用原连接，连接已删除时用当前连接）、复制 SQL、加入收藏、删除。

**收藏侧栏**
- **结构**：
  - 文件夹树，可拖拽移动和排序；
  - 顶部有"新建文件夹"按钮；
  - 收藏项显示数据库图标和名称。
- **操作**：
  - 悬停：预览 SQL 和说明；
  - 双击：插入到当前编辑器；
  - 右键：在新标签中打开（用当前连接，没有时弹出选择）、编辑（名称、SQL、说明）、重命名、新建子文件夹、删除（文件夹删除前确认）。

**收藏动作**
- Ctrl+D 或工具栏星标：收藏当前选中的 SQL，没有选中时收藏光标所在的语句。
- 弹出对话框：
  - 名称：默认取 SQL 第一行，去掉多余空白，最长 60 个字符；
  - 文件夹：树形选择，默认是上次用过的；
  - 说明：可选。

**设置**："历史"一组，包括开关、保留条数、"清空历史"按钮（确认后清空）。

所有文字用中英文词条。

## 7. 依赖

按依赖原则（M1b2 设计文档 3.1 节），实施计划的第一个任务是依赖复查（`node scripts/check-deps.mjs`）。本里程碑不引入新依赖：
- 树和拖拽用 antd 的 `Tree`；
- uuid 用后端已有的 `uuid`。

## 8. 错误处理

- 历史写入失败（磁盘满、库被锁）只记日志，不影响执行。
- 历史和收藏的查询失败时，在侧栏中显示错误文字和"重试"。
- 收藏的增删改失败时用全局提示，界面恢复原状。

## 9. 测试

- **存储层**（dblens-service）：
  - 迁移到版本 3；
  - 历史的写入、分页、搜索、过滤、清理；
  - 收藏的增删改、移动、排序、级联删除，以及防止把文件夹移进自己的下级。
- **会话**：用假驱动测试执行后写历史，覆盖成功、失败、取消、需要确认被取消、执行计划、关闭历史、行数统计。
- **接口**：沿用 `dblens-api` 的接口测试写法。
- **前端**（Vitest）：
  - 历史的日期分组；
  - 收藏的树组装、移动合法性、拖拽后的位置计算；
  - 收藏对话框的默认名称；
  - 词条中英一致（现有测试）。
- **实测**：
  - 浏览器模式下操作两个侧栏、Ctrl+D、设置项，中英文界面各一遍；
  - 实库执行语句后检查历史中的行数、耗时、错误。

## 10. 交付

- `docs/notes/m1c1-history-favorites.md` 说明文档；
- 接口说明 `docs/notes/m0-4b-api.md` 补充历史和收藏两节；
- M1 设计文档的进度中加入 M1c 的拆分和"M1c1 已完成"。
