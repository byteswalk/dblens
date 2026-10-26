# M1i 中英双语与数据库图标：设计

**上位文档**：`docs/superpowers/specs/2026-10-25-m1-workbench-design.md`。本文档记录 2026-10-25 与用户确认的决定。

**位置**：M1b1 完成之后、M1b2 开始之前插入的小里程碑。原因：界面里写死的中文已有约 244 处，分布在前端 34 个文件，后端约 160 处提示也是写死的中文。M1b2 和 M1c 还会加大量文案，越晚改成本越高。竞品调研（`docs/research/reports/数据库工具竞品与差异化.md`）把双语和数据库图标列为必须先补齐的基础能力。

## 1. 已确认的决定

| 项 | 决定 |
|---|---|
| 语言 | 简体中文 `zh-CN` 和英文 `en`；结构上可以再加其他语言 |
| 默认语言 | 跟随系统（浏览器或 WebView 的语言），设置里可手动切换，切换后立即生效 |
| 后端文字 | 后端也本地化：消息以"键 + 参数"保存，到接口层按请求语言渲染（延迟渲染） |
| 图标风格 | 有开源矢量图的用官方 logo（simple-icons）；其余用品牌主色加统一字标，不复刻对方的图形 logo |

## 2. 范围

翻译：
- 前端全部界面文字；
- antd 组件自带的文字（切换 `zh_CN` / `en_US` 语言包）；
- Monaco 自带的菜单、查找框等，用 Monaco 官方的 `esm/vs/nls/lang/zh-cn.js`；
- 后端给用户看的文字：错误、生产确认原因、驱动不可用提示、桌面版更新对话框、`dblens-server` 命令行的用法和启动提示。

不翻译：
- 数据库返回的报错原文；
- 日志（`tracing` 输出）、文档、代码注释、测试里的断言说明；
- 数据库对象名、用户数据。

## 3. 语言设置

- 设置项 `ui.lang`，取值 `auto`（默认）、`zh-CN`、`en`，放在设置对话框的"常规"里。
- `auto` 时按 `navigator.languages` 取第一个能匹配的语言：以 `zh` 开头的取中文，其余取英文。
- 切换后界面文字立即刷新。Monaco 自带文字在加载编辑器之前就要确定，所以下次启动才生效，设置项下面写明这一点。
- 窗口标题、HTML `lang` 属性随语言变化。产品名中文环境为"DBLens 数洞"，英文环境为"DBLens"。

## 4. 前端

- 库：`i18next` + `react-i18next`。
- 词条文件：`web/src/locales/zh-CN.json` 和 `web/src/locales/en.json`，按功能分组的嵌套键，如 `editor.toolbar.runCurrent`、`results.rows`。
- 类型：用 i18next 的类型声明，把 `zh-CN.json` 作为键的来源，写错键名时 `tsc` 报错。
- 组件里用 `useTranslation()`；纯逻辑模块（如 `menus.ts`、`results.ts`）用 `i18n.t`，语言变化时由调用方重新计算。
- 数字、日期、耗时用 `Intl` 按当前语言格式化。
- 请求带语言：
  - REST 请求加 `Accept-Language` 头；
  - WebSocket 连接地址加 `lang=<语言>` 参数。
- 测试：两个词条文件的键集合必须一致，每个词条的插值变量（`{{x}}`）也必须一致。

## 5. 后端：延迟渲染

### 5.1 消息类型

`dblens-core` 新增消息类型 `Msg`：
- `Msg::Text { key, args }`：词条键加命名参数，参数值都是字符串；
- `Msg::Raw(String)`：原样文本，用于数据库报错原文等不翻译的内容。

构造用宏 `msg!("not_found.session", id = 7)`；数据库原文用 `Msg::raw(e.to_string())`。

`Error` 里原来存中文字符串的字段（`message`、`what`、`hint`、`reason`）改为 `Msg`。生产确认事件 `ConfirmRequired` 的 `reason` 也改为 `Msg`。`Error::Sql` 的 `message` 是数据库原文，用 `Msg::Raw`。

### 5.2 渲染

- 词条用 `rust-i18n`，文件集中放在 `crates/dblens-core/locales/`，只有 `dblens-core` 负责渲染。
- `Msg` 序列化时按"当前渲染语言"输出成字符串。所以接口的 JSON 格式不变，`message` 等字段仍是字符串。
- 当前渲染语言是一个线程局部变量。接口层在编码响应和 WebSocket 事件时，用 `with_locale(lang, …)` 包住序列化；序列化在接口层同步完成，不跨线程。
- 请求语言：
  - REST 读 `Accept-Language`；
  - WebSocket 读连接参数 `lang`；
  - 都没有时用默认语言。
- 默认语言用于 `Display`（日志）和没有请求语言的场合：
  - 桌面版用设置 `ui.lang`，`auto` 时用系统语言；
  - 服务器用系统语言（`sys-locale`）。
- 键找不到时输出键名本身，测试保证不会发生。

### 5.3 测试

- 扫描各 crate 源码里的 `msg!("…")`，确认每个键在两种语言的词条里都存在、参数名一致。
- 同一个错误按 `zh-CN` 和 `en` 渲染，得到不同文字。
- 接口测试：带不同的 `Accept-Language` 和 `lang`，返回对应语言的错误文字和确认原因。

## 6. 数据库图标

| 变体 | 来源 |
|---|---|
| MySQL、MariaDB、PostgreSQL、TiDB | simple-icons（CC0），按需引入单个图标，不打包全量 |
| Oracle、OceanBase、PolarDB、KingbaseES、openGauss、GaussDB、HighGo、达梦 | 自绘字标：统一圆角方形底板，品牌主色，缩写字母（如 ORA、OB、PDB、KES、oG、GS、HG、DM） |

说明：
- simple-icons 16.35.0 只有上面 4 个；Oracle 已从中移除，国产库都没有（2026-10-25 核实）。
- 图标用于标识连接类型，属于指示性使用；字标不复刻对方的图形 logo。
- 暗色主题下，品牌色太深的图标（如 MariaDB `#003545`）换用提亮的颜色，保证对比度。
- 位置：对象树的连接节点、新建连接对话框的类型列表、查询标签、状态栏。

### 6.1 记住识别出的变体

- 连接配置新增可选字段 `variant`：上次连接成功时识别出的变体，由后端在测试连接或打开会话成功时写入，用户不能编辑。
- 显示规则：有 `variant` 时显示具体变体的图标，没有时显示大类图标（MySQL 系、PG 系、Oracle、达梦）。
- 修改连接时，后端忽略请求里的 `variant`，保留原值；主机、端口或驱动变了则清空，下次连接再识别。

## 7. 验收

- 自动测试全部通过：词条一致性、键覆盖、渲染、接口语言。
- 浏览器模式和桌面版各切换一次中英文，逐个面板截图检查：
  - 没有漏翻；
  - 英文较长时不撑破布局（工具栏、对话框、状态栏、树）。
- 在 5 个实库上连接后，树里显示对应变体的图标。
