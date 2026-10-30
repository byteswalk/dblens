# 打包、发布与更新（M0-4d）

## 产物

| 平台 | 产物 | 自动更新 |
|---|---|---|
| Windows x64 | `DBLens_<版本>_x64-setup.exe`（NSIS），按当前用户安装到 `%LOCALAPPDATA%\DBLens`，不需要管理员权限 | 支持 |
| macOS | 通用 dmg（Intel + Apple Silicon） | 支持 |
| Linux x64 | deb、rpm、AppImage | 只有 AppImage 支持；deb、rpm 只提示新版本 |
| Linux ARM64（信创） | deb | 只提示新版本 |
| 浏览器模式 | `dblens-server-<版本>-<目标平台>.zip/.tar.gz`（可执行文件 + `web/` 前端页面） | 只在日志中提示新版本 |

- 没有代码签名证书（设计文档第 10 节）：
  - Windows 首次运行时 SmartScreen 会提示"未知发布者"，要点"更多信息 → 仍要运行"；
  - macOS 要在"系统设置 → 隐私与安全性"中放行。
- 信创 LoongArch 留到发布阶段再落实。

## 更新签名密钥

更新包用 Tauri updater 自带的密钥对签名（minisign），与代码签名证书无关，免费。

- 公钥写在 `crates/dblens-desktop/tauri.conf.json` 的 `plugins.updater.pubkey`，随程序发布。
- 私钥只在本机打包时使用。

**私钥丢失或密码遗忘的后果**：已经发出去的版本再也收不到自动更新，只能让用户手动下载安装新版本（新版本换用新密钥）。所以私钥和密码必须离线备份。

### 生成（只做一次）

在仓库根目录执行：

```bash
pnpm --dir web tauri signer generate -w "<私钥保存路径>"
```

私钥保存路径写绝对路径，如 `C:\Users\<用户名>\.tauri\dblens-updater.key`。pnpm 在 Windows 上用 cmd 执行脚本，cmd 不会展开 `~`。

- 按提示设置密码，生成两个文件：私钥 `dblens-updater.key` 和公钥 `dblens-updater.key.pub`。
- 把 `.pub` 文件的内容（一整行 base64）填进 `tauri.conf.json` 的 `pubkey` 并提交。
- 私钥文件绝不提交到仓库。

### 备份

- 私钥文件：至少两份离线备份，例如加密 U 盘和密码管理器的附件，存放在不同地方。
- 密码：存进密码管理器，不要和私钥文件放在同一个地方。
- 每次大版本发布前，确认备份能读出来。

## Java 桥接

GBase 8s 经 Java 桥接连接，安装包要带桥接程序和精简 JRE。

- 打包机需要 JDK 11 及以上，`jlink` 用它的 jmods。**发布用 OpenJDK 构建（如 Eclipse Temurin）**，不要用 Oracle JDK，后者的再分发条款不同。
- `pnpm package` 先运行 `scripts/build-bridge.mjs`、`scripts/prepare-clients.mjs`，生成被 git 忽略的 `clients-staging/`，再打进安装包。
- 这个目录按当前平台生成，每个平台要各打一次。
- `dblens-server` 的压缩包要把 `clients-staging/` 改名为 `clients/`，放到程序旁边。
- JDBC 驱动不随包发布。只有确认许可后，才把它写进 `scripts/clients.manifest.json` 的 `jdbc`（文件名和 SHA-256），文件放在 `DBLENS_CLIENTS_DIR`。

## 发布流程

不使用 GitHub Actions：在本机检查、打包，再上传到 GitHub。这台 Windows 机器只能打 Windows 的包，其他平台要在对应系统上打。

1. 改版本号：
   - 根 `Cargo.toml` 的 `[workspace.package] version`，桌面程序和 `dblens-server` 都用它；
   - `web/package.json` 的 `version` 保持一致。
2. 写发布说明 `docs/releases/v<版本>.md`，写给用户看，简短列出变化。客户端提示更新时会显示。
3. 本地检查：`cargo fmt --all --check`、`cargo clippy --workspace --all-targets -- -D warnings`、`cargo test --workspace`，
   `web/` 下的 `pnpm typecheck`、`pnpm test`，以及实库测试。
4. 打包：设置 `TAURI_SIGNING_PRIVATE_KEY`、`TAURI_SIGNING_PRIVATE_KEY_PASSWORD` 后执行 `pnpm package`（见下面的"本地打包"），
   得到安装包和同名的 `.sig`。
5. 写 `latest.json`，`notes` 用发布说明的内容，`signature` 是 `.sig` 文件的内容：

   ```json
   {
     "version": "<版本>",
     "notes": "…",
     "pub_date": "2026-10-30T00:00:00Z",
     "platforms": {
       "windows-x86_64": {
         "signature": "…",
         "url": "https://github.com/byteswalk/dblens/releases/download/v<版本>/DBLens_<版本>_x64-setup.exe"
       }
     }
   }
   ```

6. 提交后打标签并推送，再建**草稿** Release，上传安装包和 `latest.json`：

   ```bash
   git tag v<版本>
   git push origin v<版本>
   gh release create v<版本> --draft --title "DBLens v<版本>" --notes-file docs/releases/v<版本>.md <安装包> <latest.json>
   ```

7. 在 GitHub 上检查草稿：安装包齐全，`latest.json` 中有各平台的地址和签名。然后点 Publish。
   - **发布之前客户端看不到新版本**：更新源 `…/releases/latest/download/latest.json` 只指向已发布的最新 Release。
8. 验证：用上一版本的程序点"帮助 → 检查更新…"，应提示新版本。

## 更新源

- 默认：`https://github.com/byteswalk/dblens/releases/latest/download/latest.json`。
  - 桌面版写在 `tauri.conf.json`；
  - `dblens-server` 写在 `crates/dblens-server/src/update.rs`。
- 内网部署：
  1. 把 `latest.json` 和安装包放到内网的静态文件服务器上；
  2. 改 `latest.json` 中各平台的 `url`，指向内网地址。签名不用改，它只和安装包的内容有关。
  3. 桌面版：在数据目录（Windows 为 `%APPDATA%\DBLens`）下建 `update.json`：

     ```json
     { "endpoint": "https://内网地址/dblens/latest.json" }
     ```

  4. `dblens-server`：启动时加 `--update-url https://内网地址/dblens/latest.json`。不想检查时加 `--no-update-check`。
- 关闭桌面版的自动检查：菜单"帮助 → 启动时自动检查更新"取消勾选，或在 `update.json` 中写 `"auto_check": false`。

## 本地打包（调试用）

```bash
cd web
pnpm install
pnpm package
```

- `pnpm package` 等于在仓库根执行 `tauri build --config crates/dblens-desktop/tauri.bundle.json`：
  - 先构建前端；
  - 再把 `web/dist` 作为资源打进安装包；
  - 输出到 `target/release/bundle/`。
- `tauri.bundle.json` 只在打包时传入。如果写进 `tauri.conf.json`，普通的 `cargo build` 也会检查 `web/dist` 是否存在，没构建过前端就会失败。
- 因为开启了 `createUpdaterArtifacts`，打包时要提供签名私钥：
  - 设置环境变量 `TAURI_SIGNING_PRIVATE_KEY`（私钥文件路径或内容）和 `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`；
  - 只想要安装包、不要更新签名时，加 `-- --config "{\"bundle\":{\"createUpdaterArtifacts\":false}}"`。
- 首次打包时，Tauri 会从 GitHub 下载 NSIS。
- 本机的 cargo 镜像问题（见 `m0-4c-desktop.md`）在打包时同样存在。`--` 后面的参数会交给 cargo，所以这样写：

  ```bash
  pnpm package -- --config "source.mirror.registry='sparse+https://rsproxy.cn/index/'"
  ```

- 临时覆盖配置（如换测试公钥）要用第二个 `--config <文件>`，例如 `pnpm package --config <文件>`，不能用环境变量 `TAURI_CONFIG`。CLI 会用 `TAURI_CONFIG` 传递 `tauri.bundle.json`，覆盖掉你设的值（实测）。

## 实测（Windows 11，测试密钥，本机静态服务器充当更新源）

准备计划时和执行 M0-4d 时各做了一遍：
- 执行时仓库中写的是正式公钥（key id 29D5AE699362446B），测试包用 `--config` 换成测试公钥；
- 0.1.1 安装包用测试私钥签名；
- 测试密钥用完后已删除。

1. **按当前用户安装**：`DBLens_0.1.0_x64-setup.exe /S`。
   - 装到 `%LOCALAPPDATA%\DBLens`，主程序为 `DBLens.exe`，前端页面在 `web/`；
   - 卸载信息写在 HKCU 下，不需要管理员权限。
2. **完整的自动更新流程**：
   - 0.1.0 启动 5 秒后读取 `latest.json`，弹窗显示"发现新版本 0.1.1（当前 0.1.0）"和更新说明；
   - 点"下载并安装"后下载、校验签名、以 passive 模式安装，再自动重启为 0.1.1；
   - 数据目录和窗口状态保留，注册表中的版本变为 0.1.1。
3. **签名不对时拒绝安装**：把 `latest.json` 中的签名换成另一个文件的签名后，弹出"安装更新失败：安装包签名校验失败……"，版本保持 0.1.0。
4. **"帮助"菜单**：有"检查更新…""启动时自动检查更新""关于 DBLens"三项。取消勾选自动检查后，`update.json` 中立即写入 `"auto_check": false`，`endpoint` 保留。
5. **卸载**：`uninstall.exe /S` 删除程序文件和注册表项，数据目录保留。
6. **更新源必须是 https**：
   - 正式配置下 `http://` 更新源会被拒绝（"更新源必须使用 https 地址"），内网更新源也要用 https；
   - 实测时 0.1.0 是用测试配置（`dangerousInsecureTransportProtocol: true`）打的包，只为了能用本机的 http 服务器做更新源。
7. **dblens-server 访问默认更新源**：HTTPS 正常，仓库建好之前返回 404，日志中记一条警告。
8. **测试环境的特殊情况**：从 Claude 桌面应用（MSIX 打包）中启动安装程序时，写入 `%LOCALAPPDATA%` 的文件会被重定向到该应用的包目录下。这是测试环境本身的问题，普通安装不受影响。

## 实现中发现的问题

- **MySQL 的 TLS 连接会 panic。** 更新插件依赖 rustls 的 ring 实现，工作区里原有的是 aws-lc-rs 实现。两者同时编进同一个程序后，rustls 选不出默认实现，mysql_async 一建 TLS 连接就 panic。PG 驱动指定了实现，不受影响。
  - 影响范围：全工作区测试中 MySQL 的 TLS 实库测试必现；桌面版启动后 5 秒内（更新检查之前）用 TLS 连 MySQL 也会触发。
  - 处理：MySQL 驱动在使用 TLS 前先安装 aws-lc-rs 作为进程默认实现。
