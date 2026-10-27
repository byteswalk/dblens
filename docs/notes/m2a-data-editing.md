# M2a 表数据编辑

依据：设计文档 `docs/superpowers/specs/2026-10-26-m2a-data-editing-design.md`。M2 的第一份（M2b 导入在后）。

## 结构

```text
crates/dblens-dialect/src/changes.rs   改动集（RowChange）、行标识（row_key）、按改动生成语句（执行用占位符、预览用字面量）
crates/dblens-service/src/
  session.rs     Command::Apply：在会话中按事务执行，检查影响行数，失败整体回滚；ApplyOutcome / ApplyFailure
  changes.rs     ChangeSet；Service::preview_changes / apply_changes（取表的结构模型、生成语句）
crates/dblens-api/src/routes.rs        /profiles/{id}/changes/preview、/sessions/{id}/changes
web/src/features/data/
  parse.ts       按逻辑类型解析文本；解析制表符分隔的文本
  edit.ts        编辑状态（修改、新增、删除）、粘贴区域、生成改动集
  editable.ts    可编辑的列、行标识
  editStore.ts   每个数据标签的编辑状态
  EditableGrid.tsx   可编辑表格；ChangePreview.tsx 提交前的预览
  DataView.tsx   编辑工具栏、放弃确认、提交流程
web/src/features/editor/{runner.ts, actions.tsx}   Runner.stop；关闭有未提交编辑的标签前确认
```

## 生成与执行

- 行标识：主键；没有主键时取第一个所有列都 NOT NULL 的唯一键；都没有时整张表只读。
- UPDATE 的 WHERE：行标识列 = 原值，加上改动列 = 原值（NULL 用 `IS NULL`）。不能可靠比较相等的列不参与：逻辑类型为 Float、Text、Blob、Binary、VarBinary、Json、Other，以及原值是大字段预览的。
- INSERT：自增列和有默认值的列为 NULL 时不写；一列都不写时用各方言"只用默认值"的写法。
- 语句顺序：先删除，再修改，最后新增。
- PG 的精确小数：PG 驱动不能直接绑定 NUMERIC 参数，占位符写成 `$n::text::numeric`，值按文本传。这是写计划时实库测试发现的。
- 执行：
  - 会话已在手动事务中时拒绝；
  - 临时关闭自动提交，逐条执行，每条语句先经过执行策略：要求确认的语句直接执行（预览就是确认），拒绝的语句算失败；
  - 影响行数必须为 1：为 0 时报"已被修改或删除"，大于 1 时报行标识不唯一；
  - 全部成功才提交，否则回滚；最后恢复自动提交；
  - 每条执行过的语句写一条历史（预览形式的 SQL），回滚时都记为失败。
- 接口：执行失败也是 200，结果中 `failed` 带失败的改动序号和错误。设计文档写的是"失败时返回错误"，改成这样是为了前端按序号标出失败的行。
- 会话正在读取结果（等续取）时不能执行：前端先调 `Runner.stop()` 停下，再提交。

## 前端

- 表格：已加载的行后面接新增的行。改过的单元格、新增行、删除行、解析失败的单元格和提交失败的行用不同底色。
- 编辑：双击或回车打开编辑框（Glide Data Grid 的浮层，挂在 `#portal`）。NULL 的编辑框是空的；打开后没改就关上不算改动（否则 NULL 会变成空字符串）。
- 粘贴：Glide 解析剪贴板后交给 `onPaste`，按 `edit.ts` 的规则覆盖或新增，返回 false 阻止默认行为。
- Delete 键不清空单元格；删除行用工具栏或右键菜单。
- 复制行：以第一行选中的行为底稿，行标识列不复制。
- 有未提交的编辑时，刷新、改过滤或排序、关闭标签前确认放弃。

## 验证

- 后端：fmt、clippy 通过；全部测试在 `DBLENS_LANG=en` 下对 5 个实库通过（420 个）。
  - 实库测试（5 个库）：建带主键、唯一键、自增列、默认值的表，含中文和 NULL。一次提交新增、修改、删除，核对结果（新增行取到默认值）。另一个会话先改同一字段，再按旧值提交，整体回滚，表中数据不变，会话恢复自动提交。没有主键的表被拒绝。
  - 有一次全量运行中，达梦的导出实库测试失败一次，单独重跑和再跑全量都通过。与 M1c1、M1c3 记录的情况相同：多个测试同时连达梦时偶发失败。
- 前端：tsc、Vitest、打包通过。
- 浏览器实测（PostgreSQL）：
  - 双击或回车编辑单元格，提交按钮显示改动数；
  - 新增行并填写；
  - 预览：UPDATE 带主键和改动列的原值，INSERT 只写填了的列；
  - 执行成功后提示条数，按当前过滤和排序重新读取，编辑状态清空。
- 实测中没有覆盖（内置浏览器在窗口最小化时停止绘制，表格的画布不能交互）：粘贴、右键菜单、删除行、冲突时标红、放弃确认。冲突回滚由实库测试覆盖，粘贴区域和编辑状态有单元测试。这几项在执行计划时人工确认。
