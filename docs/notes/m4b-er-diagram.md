# M4b ER 图

依据：设计文档 `docs/superpowers/specs/2026-10-28-m4b-er-diagram-design.md`。

## 结构

```text
crates/dblens-service/src/model/
  file.rs      LogicalKey；表文件的 logical_keys；ModelTable::new
  diagram.rs   图文件（Diagram、DiagramTables、DiagramDisplay、Position）；图的列表、读（表、逻辑外键、提示）、保存、新建、改名、删除；
               表改名、删除时更新各图（on_table_changed）
  store.rs     逻辑外键的增删；表、列改名时更新逻辑外键的引用；表改名、删除时调用 diagram::on_table_changed
crates/dblens-api/src/diagrams.rs   /models/{id}/diagrams…、…/logical-keys…、/files/image
web/src/features/er/
  graph.ts       节点和连线、节点尺寸、dagre 自动布局、搜索
  TableNode.tsx  表节点（每列左右各一个连接点）
  ErView.tsx     ER 图标签：工具栏、画布、建删逻辑外键、主题图的表、保存、导出
  export.ts      html-to-image 画整张图、data URL 转 base64
  events.ts      每个 ER 图标签的编辑状态（切换标签时保留未保存的改动）；ER 图中"新建表"打开的设计器保存后把表加进这张主题图
```

入口：模型面板中 schema 下的"ER 图"节点（双击图打开；右键打开、改名、删除）、schema 右键"打开 ER 图""新建主题图…"；
对象树 schema 右键"查看 ER 图"（库图，只读）。

## 文件格式

- 图文件 `diagrams/<schema>/<图名>.yaml`：`name`、`tables`（`all` 或表名列表）、`display`（`columns` / `keys` / `names`）、
  `positions`（表名 → `{x, y}`，整数）；不认识的字段保留。默认图名为 `_all`。
- 表文件的 `logical_keys`：`name`（可省）、`columns`、`ref_schema`（可省）、`ref_table`、`ref_columns`、`comment`（可省），写在 `foreign_keys` 之后。

## 与设计文档不一致的地方

- `positions` 用块写法（每个坐标一行），`y` 写成 `"y"`：YAML 1.1 中 `y` 会被当作布尔值，serde-saphyr 自动加引号。
- 在图上拖线只建单列的逻辑外键；复合的逻辑外键在表文件中手写。
- "自动布局"菜单只有横向、纵向的全部重排；没有位置的表在打开、加表时自动排在已有部分的下方（不单设"只排新表"的菜单项）。
- 导出的"白底"只换画布背景，表节点仍按当前主题的颜色；导出不内嵌网页字体（`skipFonts`），SVG 用系统字体显示。
- `html-to-image` 固定在 1.11.11：依赖复查脚本（`scripts/check-deps.mjs`）加了"有意不升级"的名单 `HOLD`，复查时列出原因、不算可升级。
- 连线的悬停提示（约束名和列的对应）用自绘的浮层，不用 React Flow 的标签。
- 库图的标签绑定连接（标签带环境色）；模型图的标签不绑定连接。
- 主题图另取默认图的全部表作为可选的表（"添加表…"、加进来的表不必先保存就能画出来），按图中的表过滤后画出。
- 关闭有未保存布局改动的 ER 图标签时先确认放弃（与设计器相同）。

## 验证

- 后端：fmt、clippy 通过；全部测试在 `DBLENS_LANG=en` 下通过（688 个）。
  - service：逻辑外键的校验、设计器保存后保留、表和列改名时引用跟着改、删除；图文件的写法（快照）、默认图没有文件、指纹冲突、手写字段保留；
    主题图的新建、改名、删除，表改名、删除时图跟着改；缺表和引用失效的逻辑外键进提示；文件有错的表只有名字。
  - api：没有 `local_files` 时禁止；图和逻辑外键的完整流程；导出图片只写 `.svg`、`.png`。
  - 实库：模型测试中同步之后再反向生成，核对默认图中外键的列对应（见下）。
- 前端：tsc、Vitest（节点和连线：复合外键、自引用、跨 schema、主题图外的表、逻辑外键；显示方式；布局；搜索；导出的 base64 和文件名；ER 图标签的保存与恢复；编辑状态与新建表的通知）、打包通过。
- 浏览器实测（模型由接口建立）：模型面板的"ER 图"节点；默认图的自动布局、列级连线、复合外键标"2 列"、自引用、跨 schema 列在节点上；
  从列拖到列建逻辑外键（虚线）；拖动表后"未保存"标记、Ctrl+S 保存、图文件内容核对；导出 PNG（整张图）；
  主题图只画图中的表、图外的引用列在节点上，"添加表…"后连线出现，节点右键"从图中移除"；右键画布"新建表"，设计器保存后表加进主题图，
  切换标签再回来未保存的改动还在；双击表在设计器中改列名，回到图上列名随之更新；200 张表的模型约 0.7 秒打开。
  实测中修正：主题图"添加表…"没有可选的表；切换标签后未保存的改动和新建表的通知丢失。
- 未验证：测试虚拟机不可达，实库测试（含本里程碑新加的反向生成后默认图的检查）没有运行，库的只读 ER 图和"另存为模型"没有实测；
  浏览器面板停止绘制时悬停菜单打不开，SVG 导出（去掉内嵌字体后的大小）、200 张表的拖动流畅度没有实测；桌面程序的导出对话框没有实测。
