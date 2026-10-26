// 自绘标题栏的菜单（M1 设计文档 1.1、1.3 节）。纯数据，按当前状态生成，单独测试；点击后由 TitleBar 按 key 执行。
// macOS 用系统菜单栏，标题栏不画菜单；浏览器模式没有"窗口"菜单，"帮助"只有"关于"。

import type { ThemePref } from '../shared/theme'
import type { SideKey } from './ui'

export type MenuEntry = { key: string; label: string; shortcut?: string; checked?: boolean } | 'divider'

export interface MenuGroup {
  label: string
  items: MenuEntry[]
}

export interface MenuState {
  desktop: boolean
  alwaysOnTop: boolean
  autoCheckUpdate: boolean
  theme: ThemePref
  side: SideKey | null
}

export function buildMenus(s: MenuState): MenuGroup[] {
  const file: MenuEntry[] = [{ key: 'new-connection', label: '新建连接…', shortcut: 'Ctrl+N' }, 'divider']
  file.push({ key: 'settings', label: '设置…' })
  if (s.desktop) file.push('divider', { key: 'quit', label: '退出' })

  const side = (key: SideKey, label: string) => ({ key: `side:${key}`, label, checked: s.side === key })
  const theme = (key: ThemePref, label: string) => ({ key: `theme:${key}`, label, checked: s.theme === key })
  const view: MenuEntry[] = [
    side('db', '数据库'),
    side('history', '历史'),
    side('favorites', '收藏'),
    side('ops', '运维查询'),
    'divider',
    theme('system', '主题跟随系统'),
    theme('light', '亮色主题'),
    theme('dark', '暗色主题'),
  ]

  const groups: MenuGroup[] = [
    { label: '文件', items: file },
    { label: '查看', items: view },
  ]
  if (s.desktop) {
    groups.push({
      label: '窗口',
      items: [
        { key: 'window:on-top', label: '窗口置顶', shortcut: 'Ctrl+Shift+T', checked: s.alwaysOnTop },
        { key: 'window:reset', label: '恢复默认窗口位置和大小' },
      ],
    })
    groups.push({
      label: '帮助',
      items: [
        { key: 'help:check-update', label: '检查更新…' },
        { key: 'help:auto-check', label: '启动时自动检查更新', checked: s.autoCheckUpdate },
        'divider',
        { key: 'help:about', label: '关于 DBLens' },
      ],
    })
  } else {
    groups.push({ label: '帮助', items: [{ key: 'help:about', label: '关于 DBLens' }] })
  }
  return groups
}
