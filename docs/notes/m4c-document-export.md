# M4c 文档导出

依据：设计文档 `docs/superpowers/specs/2026-10-28-m4c-document-export-design.md`。

## 结构

```text
crates/dblens-service/src/document/
  content.rs   文档内容 DocContent：从模型或库的结构（SourceSchema）组织表清单、明细、被引用、表间关系、统计、视图；PNG 尺寸
  labels.rs    文档中的固定文字（labels.json，中英文）：模板用 t('键')，docx、xlsx 用 label
  template.rs  html、markdown 模板（minijinja）：内置模板、过滤器 v / cell、检查语法、渲染
  templates/   内置模板 document.html、document.md（两种文档共用，按 kind 分支）
  docx.rs      docx：样式来源的读取和校验、按 Word 内部名找样式（缺的补上）、正文、目录字段、书签和链接、图片、打包
  docx/        内置样式来源的各个部件（运行时打包成 .docx）
  xlsx.rs      数据字典的 xlsx：表清单、列、索引和约束三张工作表
  job.rs       DocumentRequest、开始前检查、后台任务（TaskKind::Document）、写到本机路径或任务目录、markdown 图片目录或 zip、另存内置模板
crates/dblens-api/src/documents.rs   POST /documents、/documents/check、/documents/templates/export
web/src/features/document/
  options.ts          种类与格式的联动、按来源记住的选项、组成请求、文件名、图片的像素倍数
  ExportDocDialog.tsx 导出文档对话框
  render.tsx          屏幕外画 ER 图图片并上传（懒加载）
  track.tsx           任务进度通知；完成后下载（浏览器版）或在文件夹中显示（桌面版）
  dialog.ts           对话框的打开状态
web/src/features/er/flow.ts    图转成 React Flow 的节点和连线（ER 图标签和导出文档共用）
web/src/features/er/limits.ts  库图只显示表名的阈值
```

入口：模型面板中模型、schema 右键"导出文档…"；对象树 schema 右键"导出文档…"；ER 图工具栏"导出"菜单的"导出设计文档…"（范围为当前的图）。

## 与设计文档不一致的地方

- 内置模板 html、markdown 各一份（两种文档共用，按 `kind` 分支），不是四份；"另存内置模板"存出的一份对两种文档都适用。
- 模板的固定文字放在 `document/labels.json`（键 `doc.*`），不放进后端词条：词条检查要求每个键都被 `msg!` 用到，模板中的键由模板引用；
  测试检查内置模板和 docx、xlsx 代码中用到的键都存在、`labels.json` 中没有用不到的键。
- 生成列的表达式写在"默认值"一列，标记为"生成列"。
- docx 的图片按 2 倍像素画，按 1/192 英寸一个像素换算尺寸，再按版心缩放；样式来源中缺的样式补上时编号加 `DBLens` 前缀，基于的样式去掉（避免依赖缺的样式）。
- docx 的目录包括标题 1、2（每张表一项）；标题 1 前分页。
- 依赖：`zip` 9.0.5（`default-features = false`，只开 `deflate-flate2`；写计划时 9.1.x 发布不满 7 天，用 `cargo update --precise` 锁定）、`quick-xml` 0.42.0；
  service 的 `minijinja` 开 `serde` 特性。
- `document` 模块在 `dblens_service` 中公开（`dblens_service::document::…`），不从 crate 根重新导出。

## 验证

- 后端：fmt、clippy 通过；全部测试在 `DBLENS_LANG=en` 下通过（704 个）。
  - service：内容组织（复合外键、跨 schema 的链接、逻辑外键、被引用、读不出的表、按表筛选、视图定义开关）；
    markdown、html 的快照（中英文）、HTML 转义和内嵌图片、自定义模板的过滤器、语法错误的行号、模板不能 include 文件；
    docx 的正文快照、图片的关系和内容类型和尺寸、打开时更新字段、全部 XML 部件格式正确、中文版 Word 样式编号的样式来源、缺的样式补上、横向纸张的版心宽度、
    无效的样式来源、控制字符；xlsx 的工作表、链接、截断；开始前检查（xlsx 只用于数据字典、浏览器模式不接受本机路径、模板语法、样式来源、输出目录）；
    模型导出 markdown 到本机路径（图片在旁边的目录）、浏览器模式 markdown 打包 zip、xlsx 下载。
  - api：没有 `local_files` 时另存内置模板被禁止、本机路径作为检查出的问题；用另存的内置模板导出数据字典到本机路径。
  - 实库：11 个库的模型测试中，对测试 schema 导出 markdown 数据字典，核对表和外键的链接。
- 前端：tsc、Vitest（种类与格式的联动、记住的选项、组成请求、文件名、像素倍数）、打包通过。
- 写计划时实测：docx 由 Word 打开、更新目录、导出 PDF 后逐页核对（标题、目录页码、图片缩放、表格、链接）；xlsx 由 Excel 打开（三张工作表、链接）；
  浏览器中模型导出 HTML 设计文档（ER 图浅色白底、列级连线、"2 列"、逻辑外键虚线）、库导出 xlsx 数据字典。
- 未验证：桌面程序的保存、选择文件对话框，"在文件夹中显示"；docx 套用用户自己的样式来源后在 WPS 中的效果。
