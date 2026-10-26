// 自绘标题栏的菜单（M1 设计文档 1.1、1.3 节）。纯数据，按当前状态生成，单独测试；点击后由 TitleBar 按 key 执行。
// "SQL"菜单的项（sql:*）转成编辑器命令，由当前查询标签执行。
// macOS 用系统菜单栏，标题栏不画菜单；浏览器模式没有"窗口"菜单，"帮助"只有"关于"。
// 文字在调用时按当前语言取，切换语言后重新生成即可。

import { t } from '../shared/i18n'
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
  const file: MenuEntry[] = [
    { key: 'new-connection', label: t('app.menu.file.newConnection'), shortcut: 'Ctrl+N' },
    { key: 'new-query', label: t('app.menu.file.newQuery'), shortcut: 'Ctrl+T' },
    { key: 'close-tab', label: t('app.menu.file.closeTab'), shortcut: 'Ctrl+W' },
    'divider',
  ]
  file.push({ key: 'settings', label: t('app.menu.file.settings') })
  if (s.desktop) file.push('divider', { key: 'quit', label: t('app.menu.file.quit') })

  const side = (key: SideKey) => ({ key: `side:${key}`, label: t(`app.side.${key}` as const), checked: s.side === key })
  const theme = (key: ThemePref, label: string) => ({ key: `theme:${key}`, label, checked: s.theme === key })
  const view: MenuEntry[] = [
    side('db'),
    side('history'),
    side('favorites'),
    side('ops'),
    'divider',
    theme('system', t('app.menu.view.themeSystem')),
    theme('light', t('app.menu.view.themeLight')),
    theme('dark', t('app.menu.view.themeDark')),
  ]

  const sql: MenuEntry[] = [
    { key: 'sql:run-current', label: t('app.menu.sql.runCurrent'), shortcut: 'Ctrl+Enter' },
    { key: 'sql:run-all', label: t('app.menu.sql.runAll'), shortcut: 'F5' },
    { key: 'sql:cancel', label: t('app.menu.sql.cancel'), shortcut: 'Ctrl+Break' },
    'divider',
    { key: 'sql:format', label: t('app.menu.sql.format'), shortcut: 'Shift+Alt+F' },
    { key: 'sql:explain', label: t('app.menu.sql.explain') },
  ]

  const groups: MenuGroup[] = [
    { label: t('app.menu.file.title'), items: file },
    { label: t('app.menu.view.title'), items: view },
    { label: 'SQL', items: sql },
  ]
  if (s.desktop) {
    groups.push({
      label: t('app.menu.window.title'),
      items: [
        { key: 'window:on-top', label: t('app.menu.window.onTop'), shortcut: 'Ctrl+Shift+T', checked: s.alwaysOnTop },
        { key: 'window:reset', label: t('app.menu.window.reset') },
      ],
    })
    groups.push({
      label: t('app.menu.help.title'),
      items: [
        { key: 'help:check-update', label: t('app.menu.help.checkUpdate') },
        { key: 'help:auto-check', label: t('app.menu.help.autoCheck'), checked: s.autoCheckUpdate },
        'divider',
        { key: 'help:about', label: t('app.menu.help.about') },
      ],
    })
  } else {
    groups.push({ label: t('app.menu.help.title'), items: [{ key: 'help:about', label: t('app.menu.help.about') }] })
  }
  return groups
}
