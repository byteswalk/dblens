# M1i 中英双语与数据库图标

依据：设计文档 `docs/superpowers/specs/2026-10-25-m1i-i18n-icons-design.md`。M1b1 之后、M1b2 之前插入。

## 结构

```text
crates/dblens-core/
  src/lang.rs             语言：请求范围内的当前语言（task_local）、进程默认语言（设置 ui.lang / DBLENS_LANG / 系统）
  src/msg.rs              Msg（词条键 + 参数，或原文）、msg! 宏；序列化时按当前语言渲染
  src/error.rs            Error 的文字字段都是 Msg；Display 按当前语言
  locales/app.json        全部后端词条，每条一行，含中英文
  build.rs                词条文件变化时重新编译（rust-i18n 在编译时嵌入词条）
  tests/catalog.rs        词条检查：键存在、参数一致、没有多余词条、源码里没有给用户看的中文
crates/dblens-api/src/lang.rs   请求语言：查询参数 lang 优先，其次 Accept-Language
crates/dblens-service/          Service::apply_lang_setting；Profile.variant（迁移版本 2）
web/src/
  shared/i18n.ts          i18next 初始化、语言设置 ui.lang、currentLang / applyLang / t
  shared/i18next.d.ts     词条键的类型，以中文词条为准
  locales/<语言>/<模块>.json   前端词条；locales.test.ts 检查中英文一致
  features/editor/monacoLocale.ts   Monaco 自带文字的中文包，在加载编辑器前载入
  shared/dbIcons.ts · shared/DbIcon.tsx   数据库图标
```

## 选型复核

2026-10-25 查询：

| 库 | 版本 | 说明 |
|---|---|---|
| `rust-i18n` | 4.2.4 | 编译时嵌入词条，支持按语言取词条和回退 |
| `sys-locale` | 0.3.2 | 读取操作系统语言 |
| `i18next` | 26.4.3 | 2026-10-10 发版 |
| `react-i18next` | 17.0.16 | 2026-10-06 发版 |
| `simple-icons` | 16.35.0 | CC0；2026-10-11 发版，按需引入，打包后只含用到的 4 个图标 |

前端依赖都选了发布满两周的版本。

## 怎么加一条文字

后端：
- 写 `msg!("模块.名称", 参数 = 值)`，在 `crates/dblens-core/locales/app.json` 里加一行中英文。
- 参数可以是实现了 `Display` 的值，也可以是另一个 `Msg`（与外层按同一种语言翻译）。
- 数据库返回的原文用 `Msg::raw(…)`，不翻译。
- `cargo test -p dblens-core --test catalog` 会检查键、参数和源码里残留的中文。

前端：
- 组件里 `const { t } = useTranslation()`；不是组件的模块 `import { t } from '../shared/i18n'`，并且在调用时才翻译，不能在模块加载时缓存。
- 词条加在 `locales/zh-CN/<模块>.json` 和 `locales/en/<模块>.json`，键名写错时 `tsc` 报错。
- 英文要区分单复数时用 i18next 的 `_one` / `_other` 后缀，传 `count`；中文只写一条。

## 语言怎么决定

- 前端：设置 `ui.lang`（`auto` / `zh-CN` / `en`），`auto` 时按浏览器首选语言，`zh` 开头的取中文，其余取英文。
- 前端发请求时带上当前语言：REST 用 `Accept-Language`，WebSocket 用地址参数 `lang`。
- 后端在接口层用这个语言渲染响应和事件里的文字，所以 JSON 格式不变。
- 没有请求语言的场合用默认语言：日志、`dblens-server` 的命令行输出、桌面版的更新对话框和 macOS 菜单。
  - 按设置 `ui.lang`；为 `auto` 时看环境变量 `DBLENS_LANG`，再看系统语言。
- 不翻译：数据库自身的报错原文、日志、文档和注释。

## 数据库图标

| 变体 | 图标 |
|---|---|
| MySQL、MariaDB、PostgreSQL、TiDB | simple-icons 的官方 logo |
| Oracle、OceanBase、PolarDB、KingbaseES、openGauss、GaussDB、HighGo、达梦 | 品牌主色的圆角方块加缩写：ORA、OB、PDB、KES、oG、GS、HG、DM |

- 位置：对象树的连接节点、新建连接对话框、查询标签、状态栏。
- 连接配置新增只读字段 `variant`：后端在连接成功时写入；改了主机、端口或驱动后清空。
- 没连接过的配置显示驱动大类的图标；对象树第一次展开连接后会重新取连接列表，换成具体数据库的图标。
- 暗色主题下 MariaDB 的深色 logo 换用提亮的颜色。
- 字标的颜色是按各家常用主色选的近似值，以后可以在 `shared/dbIcons.ts` 里调整。

## 实现中发现的问题

- rust-i18n 在编译时嵌入词条，但不会因为词条文件变化而重新编译。需要 `dblens-core/build.rs` 声明 `rerun-if-changed=locales`，否则改了词条后渲染出来的还是键名。
- `Msg` 有意不实现 `Display`，避免被当作普通文字提前按默认语言渲染。日志等场合显式调用 `.current()`。
- 当前语言放在 `tokio::task_local` 中。
  - 接口层的中间件包住整个请求，令牌错误也按请求语言输出。
  - WebSocket 升级后的连接和续取任务运行在新任务里，要用 `scope_lang` 把语言带过去。
- PG 驱动在 `FromSql` / `ToSql` 里返回的错误会被 tokio-postgres 包一层，要从 `source()` 中取回原来的 `Error`。否则提示变成英文的包装文字。
- Monaco 自带文字的语言必须在 `monaco-editor` 加载前设定，所以外壳在懒加载工作区之前先载入中文包。切换语言后要下次启动才生效，设置里有说明。
- 生成 SQL 中的注释（Oracle 没有可插入列时的 INSERT）按请求语言生成。
- 测试：
  - 前端测试默认用中文（`src/test-setup.ts`）。
  - 后端的命令行测试给子进程设 `DBLENS_LANG=zh-CN`。
  - 后端全部测试在 `DBLENS_LANG=en` 下也要通过，确保没有测试依赖系统语言。
- 已经产生的文字不随语言切换：已打开编辑器的右键菜单、消息标签里已有的行、桌面程序启动时生成的窗口标题和 macOS 菜单。新产生的内容用新语言。

## 验证

- 后端：fmt、clippy 通过；全部测试在 `DBLENS_LANG=en` 下对 5 个实库通过（345 个）。
- 前端：tsc 通过；20 个测试文件、89 个测试通过；打包通过。
- 浏览器实测：
  - 中文与英文界面都检查过；在设置里切换后立即生效，只剩语言选项里的“中文”是中文。
  - 英文下对话框、工具栏、状态栏和标签没有文字溢出。
  - 5 个实库连接后，对象树显示对应数据库的图标。
