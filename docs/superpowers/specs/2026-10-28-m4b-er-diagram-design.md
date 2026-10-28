# M4b ER 图 设计

日期：2026-10-28。上级设计：总设计文档 `docs/superpowers/specs/2026-10-22-dblens-design.md` 第 11 节（M4 模型："ER 图（按列画外键连线，支持自定义逻辑外键）"）；M4a 设计 `docs/superpowers/specs/2026-10-27-m4a-model-compare-design.md`（模型目录、表文件、指纹、`/models` 接口）。

## 1. 目标与范围

- 在模型的一个 schema 上画 ER 图：每个 schema 有一张默认图（全部表），另可建多张主题图；布局存回模型目录。
- 按列画外键连线；自定义逻辑外键（库中没有建的关系），存在表文件中，不参与比对和同步。
- 图上能做的编辑：布局、逻辑外键、主题图中的表；表结构仍用 M3 设计器。
- 连接的 schema 也能看 ER 图：只读、自动布局、不保存，可另存为模型。
- 导出 SVG、PNG。

不做：
- 在图上直接改表（加列、改类型、拖线建真外键）；
- 一张图跨多个 schema（引用其他 schema 的外键只在节点上列出）；
- 库图的布局保存；
- Mermaid / PlantUML 等文本格式的导出；
- 逻辑外键转为真外键。

## 2. 已定的决策（2026-10-28 与用户确认）

| 问题 | 结论 |
|---|---|
| 范围 | 模型和库的 schema 都能画；库图只读、自动布局、布局不保存，可一键另存为模型 |
| 图的组织 | 每个 schema 可有多张图：默认图（全部表）加主题图（选中的表） |
| 逻辑外键 | 存在表文件中，不参与比对、同步；图上画虚线；M4c 文档中列出 |
| 图上编辑 | 布局、逻辑外键、主题图的表；双击表或"新建表"打开 M3 设计器 |
| 导出 | SVG、PNG |
| 方案 | 前端用 React Flow（`@xyflow/react`）画图、dagre（`@dagrejs/dagre`）自动布局、`html-to-image` 导出；后端读写图文件和逻辑外键 |

不选的方案：AntV X6（不是 React 写法，体积大）；自绘 SVG（平移缩放、拖动、大图性能都要自己做）；ELK 布局（EPL-2.0 / GPL-3.0 许可）。

## 3. 模型目录中的存储

### 3.1 图文件

`diagrams/<schema>/<图名>.yaml`，schema 名和图名的转义与表文件相同（M4a 3.1 节）：

```yaml
name: 订单
tables: [orders, order_items, customers]   # 默认图写 all，表示全部表
display: columns       # columns 全部列 / keys 只显示键列 / names 只显示表名
positions:             # 表的位置；没记位置的表打开时自动排
  orders: { x: 120, y: 40 }
  customers: { x: 480, y: 40 }
```

- 每个 schema 都有默认图：名字固定为 `_all`（文件 `_all.yaml`，界面上显示为"全部表"，主题图不能用这个名字），`tables: all`；没有文件时按全部表、自动布局打开，第一次保存时建文件。
- 主题图可新建、改名、删除；默认图不能改名、删除。
- 字段顺序固定，不认识的字段保留，先写临时文件再改名；保存前核对指纹（与 M4a 相同）。
- 表改名：各图的 `tables`、`positions` 中的名字跟着改；表删除：从各图中去掉。

### 3.2 逻辑外键

存在表文件中，与 `foreign_keys` 平级：

```yaml
logical_keys:
- name: lk_orders_customer      # 可省
  columns: [customer_code]
  ref_table: customers          # 引用其他 schema 时另写 ref_schema
  ref_columns: [code]
  comment: 历史原因没建外键      # 可省
```

- 放在服务层的模型表（`ModelTable.logical_keys`）中，不进 `dblens_core::schema::Table`：比对、同步、M3 设计器都看不到，设计器保存时原样保留。
- 表、列改名时其他表的逻辑外键引用跟着改（与真外键相同）。
- 被引用的表或列不存在时，逻辑外键保留，图上不画线，在图的提示列表中列出。
- 增删时校验：两边的列存在、列数一致、名字在本表中不重复（不给名字时不查）。

### 3.3 库的 schema

不存任何东西：每次打开按元数据自动布局；没有逻辑外键。

## 4. 界面

### 4.1 标签与入口

- 新的标签类型"ER 图"：`{model: 目录, schema, diagram: 图名}` 或 `{profile, schema}`；随标签保存、下次启动恢复。
- 模型面板：schema 下多一个"ER 图"节点，展开列出这个 schema 的图（默认图在前）；双击打开；右键图：打开、改名、删除（默认图只有打开）；右键 schema："新建主题图…""打开 ER 图"。
- 对象树：schema 右键"查看 ER 图"（库图）。

### 4.2 画布

- 表节点：表头为表名（悬停显示注释；文件有错的表标红，只显示名字）；每列一行：主键、外键图标，列名，类型，可空的列名浅色。显示方式：全部列 / 只显示键列 / 只显示表名。
- 连线：真外键实线、逻辑外键虚线，从外键列连到被引用的列；复合外键画一条（第一列之间），线上标列数；悬停显示约束名和列的对应；自引用画成回到本表的线。引用其他 schema 的表时不画线，在节点底部列出"→ crm.customers"。
- 交互：拖动表、框选后一起拖、滚轮缩放、小地图、"适应窗口"；"自动布局"（dagre，横向或纵向；只排没有位置的表或全部重排）；搜索表名后定位并高亮。
- 只渲染可见区域的节点和连线；默认图超过 300 张表时先用"只显示表名"打开。
- 图的提示列表（工具栏图标带数目）：引用不存在的逻辑外键、主题图中已不存在的表、文件有错的表。

### 4.3 模型图的编辑

- 逻辑外键：从列的连接点拖到另一张表的列（或本表的列），弹框填名称、注释后建成；选中虚线后按 Delete 或右键删除。
- 双击表打开 M3 设计器（模型模式）；右键画布"新建表"也打开设计器，保存后加入当前图（默认图本来就包含）。
- 主题图："添加表…"从这个 schema 的表中多选；节点右键"从图中移除"（不删除表）。
- 布局、显示方式、主题图的表有改动时标题带"未保存"标记，Ctrl+S 或"保存"写图文件；文件已被修改时拒绝，提示重新加载。逻辑外键的增删直接写表文件，不经过"保存"。
- 设计器保存、模型面板中改名删除表之后，打开的图重新读取（保留未保存的布局改动中仍有效的部分）。

### 4.4 库图

- 只读：可以拖动、自动布局，但不能保存；双击表打开数据标签，右键"设计表"打开 M3 设计器。
- 工具栏"另存为模型…"：打开 M4a 的反向生成对话框，带上连接和 schema。

### 4.5 导出

- 工具栏"导出 SVG""导出 PNG"：导出整张图（不是当前视口），PNG 按 2 倍像素；背景跟随当前主题，可选白底。
- 桌面程序用系统的"另存为"对话框，经 `POST /files/image` 写文件；浏览器模式下载。

## 5. 服务层与接口

### 5.1 服务层（`dblens-service/src/model/`）

- `file.rs`：表文件加 `logical_keys`（`LogicalKey { name, columns, ref_schema, ref_table, ref_columns, comment }`），读进 `ModelTable.logical_keys`；图文件 `DiagramFile { name, tables: All | List, display, positions, extra }`。
- `diagram.rs`（新）：列出一个 schema 的图（默认图没有文件时也列出）；读图（图、所含的表和各自的逻辑外键、文件有错的表、指纹、提示）；保存（核对指纹）；新建、改名、删除。
- `store.rs`：表改名时更新其他表逻辑外键的引用和各图的表名、位置；列改名时更新其他表逻辑外键引用的列；删除表时从各图中去掉；写回模型时保留逻辑外键；逻辑外键的增删（校验、核对表文件的指纹）。

### 5.2 接口（都要求 `local_files`）

| 方法 | 路径 | 说明 |
|---|---|---|
| GET | `/models/{id}/diagrams?schema=` | 图列表 `[{schema, name, tables}]` |
| POST | `/models/{id}/diagrams` | 新建主题图 `{schema, name, tables}` |
| GET | `/models/{id}/diagrams/{schema}/{name}` | `{diagram, tables: [{table, logical_keys, error?}], fingerprint, warnings}`；默认图的 `name` 为 `_all` |
| PUT | `/models/{id}/diagrams/{schema}/{name}` | 保存 `{diagram, fingerprint}`，返回新指纹 |
| DELETE | `/models/{id}/diagrams/{schema}/{name}` | 删除主题图 |
| POST | `/models/{id}/diagrams/{schema}/{name}/rename` | 主题图改名 `{to}` |
| POST | `/models/{id}/tables/{schema}/{name}/logical-keys` | 加逻辑外键 `{key, fingerprint}`，返回表的新指纹 |
| DELETE | `/models/{id}/tables/{schema}/{name}/logical-keys/{序号}?fingerprint=` | 删除逻辑外键 |
| POST | `/files/image` | 桌面程序导出图片：`{path, data}`（`data` 为 base64），只接受 `.svg`、`.png`；与 `/compare/script` 一样要求 `local_files` |

- 库图沿用 `GET /profiles/{id}/schemas/{schema}/tables`（整个 schema 的表明细），后端不改。

### 5.3 前端（`web/src/features/er/`）

- `graph.ts`（纯逻辑）：表和外键换算成节点、连线；按显示方式算节点尺寸；dagre 布局（保留已记的位置，只排新表）；跨 schema 的外键；提示列表。
- `ErView.tsx`、`TableNode.tsx`、工具栏、建逻辑外键的对话框、导出。
- 依赖：`@xyflow/react` 12.12.0、`@dagrejs/dagre` 3.1.1（3.1.2、3.1.3 发布不满 7 天）、`html-to-image`（React Flow 文档推荐 1.11.11，更新的版本有导出图片的问题）；实施计划的依赖复查中确认版本。
- ER 图标签懒加载，打开第一张图时才加载 React Flow。

## 6. 测试

- 服务层：图文件读写（快照固定写法、未知字段保留、默认图没有文件）；逻辑外键读写、设计器保存后保留；表改名、列改名时逻辑外键引用和图中的名字、位置跟着改；删除表后从图中去掉；引用不存在的逻辑外键进提示；指纹冲突；比对结果不含逻辑外键。
- 接口：没有 `local_files` 时禁止；图的新建、读、存、改名、删除，逻辑外键增删的完整流程；导出只写 `.svg`、`.png`。
- 前端：`graph.ts` 的节点和连线（复合外键、自引用、跨 schema、逻辑外键虚线）、布局（保留已有位置、只排新表）、各显示方式的节点尺寸；ER 图标签的保存与恢复。
- 实库（11 个目标）：在 M4a 的模型实库测试中，反向生成后读默认图，核对外键连线的列对应。
- 浏览器实测：模型图的自动布局、拖动后保存、重新打开位置不变；拖线建逻辑外键、删除；主题图添加、移除表；双击表打开设计器改列名后图随之更新；导出 SVG、PNG 并打开核对；库图只读与"另存为模型"；约 200 张表的 schema 拖动流畅。

## 7. 文档

- `docs/notes/m4b-er-diagram.md`：结构、图文件和逻辑外键的格式、与本设计不一致的地方。
- `docs/notes/m0-4b-api.md`：图和逻辑外键的接口。
- 总设计文档：M4b 完成后更新进度。
