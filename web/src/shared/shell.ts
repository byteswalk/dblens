// 桌面外壳的原生能力（M1 设计文档 2.2 节）：自绘标题栏的窗口操作，以及置顶、恢复默认位置、检查更新、关于、退出；
// 导出（M1c3）用的"另存为"对话框和"在文件夹中显示"，导入（M2b）用的"打开"对话框。
// 只有这里调用 Tauri；其他代码通过这里的函数使用。浏览器模式（dblens-server）没有 Tauri，desktop 为 null。

import { invoke } from '@tauri-apps/api/core'
import { listen } from '@tauri-apps/api/event'

export interface ShellInfo {
  platform: 'windows' | 'macos' | 'linux' | string
  version: string
  maximized: boolean
  always_on_top: boolean
  auto_check_update: boolean
}

export type WindowAction =
  | 'minimize'
  | 'toggle_maximize'
  | 'close'
  | 'start_dragging'
  | 'toggle_always_on_top'
  | 'reset_position'
  | 'check_update'
  | 'about'
  | 'quit'

/** 与 Rust 端 `shell::EVENT` 一致。 */
export const SHELL_EVENT = 'dblens://shell'

export interface DesktopShell {
  info(): Promise<ShellInfo>
  action(a: WindowAction): Promise<void>
  setAutoCheckUpdate(on: boolean): Promise<void>
  /** 订阅状态变化，返回取消订阅的函数。 */
  onChange(cb: (info: ShellInfo) => void): Promise<() => void>
  /** 系统"另存为"对话框；用户取消时为 null。 */
  saveDialog(defaultName: string, filters: { name: string; extensions: string[] }[]): Promise<string | null>
  revealInFolder(path: string): Promise<void>
  /** 系统"打开"对话框；用户取消时为 null。 */
  openDialog(filters: { name: string; extensions: string[] }[]): Promise<string | null>
}

/** 页面是否运行在桌面程序里：Tauri 会在页面脚本运行前注入 `__TAURI_INTERNALS__`。 */
export function isDesktop(w: object | undefined = globalThis.window): boolean {
  return w !== undefined && '__TAURI_INTERNALS__' in w
}

function createDesktopShell(): DesktopShell {
  return {
    info: () => invoke<ShellInfo>('shell_info'),
    action: (action) => invoke('window_action', { action }),
    setAutoCheckUpdate: (on) => invoke('set_auto_check_update', { on }),
    onChange: async (cb) => listen<ShellInfo>(SHELL_EVENT, (e) => cb(e.payload)),
    saveDialog: (defaultName, filters) => invoke<string | null>('save_dialog', { defaultName, filters }),
    revealInFolder: (path) => invoke('reveal_in_folder', { path }),
    openDialog: (filters) => invoke<string | null>('open_dialog', { filters }),
  }
}

export const desktop: DesktopShell | null = isDesktop() ? createDesktopShell() : null

/** 标题栏左侧要给系统按钮留的宽度：macOS 的红黄绿按钮在左上角。 */
export function titleBarInset(platform: string | undefined): number {
  return platform === 'macos' ? 76 : 0
}
