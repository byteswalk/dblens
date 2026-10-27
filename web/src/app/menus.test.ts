import { describe, expect, it } from 'vitest'

import i18n from '../shared/i18n'
import { type MenuEntry, buildMenus } from './menus'

const base = { desktop: true, alwaysOnTop: false, autoCheckUpdate: true, theme: 'system' as const, side: 'db' as const }
const keys = (items: MenuEntry[]) => items.map((i) => (i === 'divider' ? '-' : i.key))

describe('标题栏菜单', () => {
  it('桌面版有文件、查看、SQL、窗口、帮助五组', () => {
    const m = buildMenus(base)
    expect(m.map((g) => g.label)).toEqual(['文件', '查看', 'SQL', '窗口', '帮助'])
    expect(keys(m[0].items)).toEqual([
      'new-connection',
      'new-query',
      'close-tab',
      '-',
      'new-table',
      'import',
      'sql-file',
      '-',
      'settings',
      '-',
      'quit',
    ])
    expect(keys(m[2].items)).toEqual(['sql:run-current', 'sql:run-all', 'sql:cancel', '-', 'sql:format', 'sql:explain', '-', 'sql:favorite'])
    expect(keys(m[4].items)).toEqual(['help:check-update', 'help:auto-check', '-', 'help:about'])
  })

  it('浏览器模式没有窗口菜单和退出，帮助只有关于', () => {
    const m = buildMenus({ ...base, desktop: false })
    expect(m.map((g) => g.label)).toEqual(['文件', '查看', 'SQL', '帮助'])
    expect(keys(m[0].items)).toEqual(['new-connection', 'new-query', 'close-tab', '-', 'new-table', 'import', 'sql-file', '-', 'settings'])
    expect(keys(m[3].items)).toEqual(['help:about'])
  })

  it('勾选状态反映当前的侧栏、主题、置顶和自动检查', () => {
    const m = buildMenus({ ...base, alwaysOnTop: true, theme: 'dark', side: 'history', autoCheckUpdate: false })
    const checked = m.flatMap((g) => g.items).filter((i) => i !== 'divider' && i.checked)
    expect(checked.map((i) => (i as { key: string }).key)).toEqual(['side:history', 'theme:dark', 'window:on-top'])
  })

  it('文字按调用时的界面语言生成', async () => {
    await i18n.changeLanguage('en')
    try {
      const m = buildMenus(base)
      expect(m.map((g) => g.label)).toEqual(['File', 'View', 'SQL', 'Window', 'Help'])
      const labels = m.flatMap((g) => g.items).map((i) => (i === 'divider' ? '-' : i.label))
      expect(labels).toContain('Run Current Statement')
      expect(labels).toContain('Always on Top')
      expect(labels).toContain('Database')
    } finally {
      await i18n.changeLanguage('zh-CN')
    }
    expect(buildMenus(base)[0].label).toBe('文件')
  })
})
