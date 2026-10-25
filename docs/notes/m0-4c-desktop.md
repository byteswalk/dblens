# 前端外壳与桌面（M0-4c）

## 怎么运行

前端在 `web/`，包管理器用 pnpm（版本见 `web/package.json` 的 `packageManager`）。

| 场景 | 步骤 |
|---|---|
| 浏览器模式 | `cd web && pnpm install && pnpm build`；`cargo run -p dblens-server -- --web-dir web/dist`；打开它打印的"在浏览器中打开"链接 |
| 前端开发（热更新） | `cargo run -p dblens-server -- --allow-origin http://localhost:5173`；`cd web && pnpm dev`；打开 server 打印的链接，把其中的 `http://127.0.0.1:8742` 换成 `http://localhost:5173` |
| 桌面 | 先在 `web/` 下 `pnpm build`，再 `cargo run -p dblens-desktop` |

本机的 cargo 镜像：`CARGO_HOME` 中配置的阿里云镜像索引滞后，缺少 Tauri 依赖的 brotli 9.1.0，会报 `failed to select a version for the requirement brotli = "^9"`。在镜像补上之前，可以改用 rsproxy：

- 命令加 `--config "source.mirror.registry='sparse+https://rsproxy.cn/index/'"`；
- `cargo clippy` 要把这个参数写在 `clippy` 后面；
- 也可以把 `D:\Environments\cargo\config.toml` 中的镜像改成 rsproxy。

桌面程序可用的环境变量（开发和测试用）：

- `DBLENS_DATA_DIR`：数据目录，默认是用户数据目录下的 `DBLens`（Windows 为 `%APPDATA%\DBLens`）。
- `DBLENS_WEB_DIR`：前端页面目录。不设时依次找安装包资源目录下的 `web/`、调试构建时源码树中的 `web/dist`。

## 架构要点

- 前端页面由接口服务同源提供（`dblens_api::with_web`），前端调用接口不涉及跨域，接口不需要 CORS。
  - 静态文件不需要令牌，`/api` 下的接口仍然需要。
  - 没有扩展名的路径找不到文件时返回 `index.html`（单页应用）。`/api` 下不存在的路径一律 404。
- 令牌：
  - 桌面模式：Tauri 的初始化脚本在页面脚本运行前设置 `window.__DBLENS__.token`。窗口只允许停留在接口服务的地址下，防止令牌被注入其他网站的页面。
  - 浏览器模式：`dblens-server` 打印的链接带 `#token=…`。`#` 后面的内容不会发给服务器，也不会出现在访问日志和 Referer 中。前端取出后存进 sessionStorage，并从地址栏去掉。
    - server 重启后令牌会变。在已经打开页面的标签页里打开新链接时，只有 `#` 后面变化，浏览器不会重新加载页面。所以前端监听 `hashchange`，地址里带令牌时重新加载（联调中发现）。
- 桌面程序不用 Tauri IPC，前端里也没有 Tauri 相关代码。窗口置顶、恢复默认位置都走原生菜单和系统热键。

## 窗口行为（设计文档 7.2 节）

| 行为 | 实现 |
|---|---|
| 默认大小 | 主屏幕工作区内居中，1440×900 逻辑像素；某个方向放不下时，这个方向取工作区的 90%；最小 1024×640 |
| 主题 | 页面跟随系统的亮色/暗色（`prefers-color-scheme`），切换系统主题时实时生效；标题栏由系统绘制，同样跟随 |
| 置顶 | 菜单"窗口 → 窗口置顶"，快捷键 Ctrl+Shift+T |
| 恢复默认 | 菜单"窗口 → 恢复默认窗口位置和大小" |
| 记住状态 | 数据目录下的 `window.json`：未最大化时的位置和大小、是否最大化、是否置顶。允许开头有 BOM |
| 拉回 | 保存的标题栏没有至少 100×20 像素落在某个显示器的工作区内时，启动时改用主屏幕的默认位置 |

以上在 Windows 11（2560×1600，100% 缩放）上逐项实测通过。实测中发现两个问题，都已处理：

- **菜单快捷键在页面有焦点时不触发。** WebView2 运行在单独的进程里，按键不经过窗口的消息循环。改用系统热键（`tauri-plugin-global-shortcut`），窗口获得焦点时注册、失去焦点时注销，所以不影响其他程序。
  - 不用 Ctrl+Alt+T：它在 Linux 桌面上是打开终端的系统快捷键。
- **最大化时会误记位置。** 移动、缩放事件比"已最大化"标志先到，立即记录会把最大化后的大小当成普通大小。改为最后一次事件后 300ms 再记录，退出时再同步记录一次。

## 日志

- 位置：数据目录下的 `logs/dblens.YYYY-MM-DD.log`，按天滚动，保留 14 天。
- 发布版没有控制台，panic 也写进日志。调试构建同时输出到控制台。
- `dblens-server` 仍只输出到控制台（标准错误）。

## 未做 / 后续

- **M0-4d 打包与更新**：
  - 把 `web/dist` 作为资源打进安装包的 `web/` 目录（桌面程序已经会在资源目录下找）；
  - NSIS 按当前用户安装、updater 签名密钥与 `latest.json`、CI 发布矩阵；
  - `tauri.conf.json` 的 `bundle.active` 目前是 false；
  - 占位图标在 M1 设计稿确定后替换（`crates/dblens-desktop/icons/`，源图由 `pnpm dlx @tauri-apps/cli@2.12.4 icon` 生成各尺寸）。
- **M1**：
  - 正式界面要先出设计稿（设计文档 7.1 节）；
  - 设置中"固定亮色/暗色"；
  - 自定义标题栏（如需要）；
  - 前端页面目录缺失时给出提示页（目前只记日志，窗口里是 404）。
- **未实测**：
  - macOS、Linux 上的窗口行为和热键。Wayland 下 `global-hotkey` 不可用，届时菜单仍可用，但快捷键要另想办法；
  - CI 中新增的 Linux 依赖和前端任务，要等仓库有远程后才能跑。
