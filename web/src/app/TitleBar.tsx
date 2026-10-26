// 自绘标题栏（M1 设计稿第 1、5 部分）：菜单、居中标题、置顶按钮、窗口按钮。
// 空白处按下鼠标开始拖动窗口，双击最大化或还原。macOS 不画菜单（用系统菜单栏），左侧给红黄绿按钮留位置；
// 浏览器模式只有菜单和标题。

import { App, Dropdown, type MenuProps } from 'antd'
import { useQuery } from '@tanstack/react-query'
import { IconCheck, IconCopy, IconDatabase, IconMinus, IconPin, IconSquare, IconX } from '@tabler/icons-react'
import { type MouseEvent, useEffect } from 'react'

import { useQueryActions } from '../features/editor/actions'
import { type EditorCommand, sendCommand } from '../features/editor/commands'
import { useTabs } from '../features/editor/tabs'
import { useSetting, useSettings } from '../shared/settings'
import { type WindowAction, desktop, titleBarInset } from '../shared/shell'
import { THEME_KEY, type ThemePref, parseThemePref } from '../shared/theme'
import { type MenuGroup, buildMenus } from './menus'
import { useApi } from './context'
import { type SideKey, useUi } from './ui'
import { useShellInfo } from './useShellInfo'

const RELEASES = 'https://github.com/byteswalk/dblens/releases'

function toAntItems(g: MenuGroup): MenuProps['items'] {
  return g.items.map((i, n) =>
    i === 'divider'
      ? { type: 'divider', key: `d${n}` }
      : {
          key: i.key,
          label: (
            <span className="row" style={{ gap: 24, minWidth: 180 }}>
              <span className="grow">{i.label}</span>
              {i.shortcut && <span className="muted">{i.shortcut}</span>}
            </span>
          ),
          icon: <IconCheck size={14} style={{ visibility: i.checked ? 'visible' : 'hidden' }} />,
        },
  )
}

export default function TitleBar() {
  const info = useShellInfo()
  const { modal } = App.useApp()
  const ui = useUi()
  const theme = parseThemePref(useSetting(THEME_KEY))
  const act = (a: WindowAction) => desktop?.action(a).catch((e: unknown) => console.warn(a, e))
  const { newQuery, closeActive } = useQueryActions()
  const api = useApi()
  const { data: profiles } = useQuery({ queryKey: ['profiles'], queryFn: () => api.profiles() })
  const tab = useTabs((s) => s.tabs.find((t) => t.id === s.active))
  const profile = profiles?.find((p) => p.id === tab?.profileId)
  // 标题："当前标签 · 连接 — DBLens 数洞"
  const title = tab ? `${tab.title} · ${profile?.name ?? '连接已删除'} — DBLens 数洞` : 'DBLens 数洞'
  useEffect(() => {
    document.title = title
  }, [title])

  const run = (key: string) => {
    const [kind, arg] = key.split(':')
    if (key === 'new-connection') ui.openConnection(null)
    else if (key === 'new-query') newQuery()
    else if (key === 'close-tab') closeActive()
    else if (kind === 'sql') sendCommand(arg as EditorCommand)
    else if (key === 'settings') ui.setSettingsOpen(true)
    else if (key === 'quit') act('quit')
    else if (kind === 'side') ui.setSide(arg as SideKey)
    else if (kind === 'theme') useSettings.getState().set(THEME_KEY, arg as ThemePref)
    else if (key === 'window:on-top') act('toggle_always_on_top')
    else if (key === 'window:reset') act('reset_position')
    else if (key === 'help:check-update') act('check_update')
    else if (key === 'help:auto-check') desktop?.setAutoCheckUpdate(!info?.auto_check_update).catch(console.warn)
    else if (key === 'help:about') {
      if (desktop) act('about')
      else modal.info({ title: 'DBLens 数洞', content: <a href={RELEASES}>{RELEASES}</a> })
    }
  }

  const mac = info?.platform === 'macos'
  const menus = mac
    ? []
    : buildMenus({
        desktop: desktop !== null,
        alwaysOnTop: info?.always_on_top ?? false,
        autoCheckUpdate: info?.auto_check_update ?? true,
        theme,
        side: ui.side,
      })

  // 只有空白处（不是菜单和按钮）才拖动窗口
  const onMouseDown = (e: MouseEvent) => {
    if (!desktop || e.button !== 0 || (e.target as HTMLElement).closest('[data-no-drag]')) return
    act(e.detail === 2 ? 'toggle_maximize' : 'start_dragging')
  }

  return (
    <div className="row titlebar" onMouseDown={onMouseDown} style={{ paddingLeft: titleBarInset(info?.platform) }}>
      <IconDatabase size={16} className="logo" />
      <div className="row" data-no-drag>
        {menus.map((g) => (
          <Dropdown key={g.label} trigger={['click']} menu={{ items: toAntItems(g), onClick: (e) => run(e.key) }}>
            <span className="menu-btn">{g.label}</span>
          </Dropdown>
        ))}
      </div>
      <span className="title">{title}</span>
      <span className="grow" />
      {desktop && (
        <div className="row" data-no-drag>
          <span
            className={`tb-btn${info?.always_on_top ? ' on' : ''}`}
            title="窗口置顶（Ctrl+Shift+T）"
            onClick={() => act('toggle_always_on_top')}
          >
            <IconPin size={16} />
          </span>
          {!mac && (
            <>
              <span className="tb-btn" title="最小化" onClick={() => act('minimize')}>
                <IconMinus size={16} />
              </span>
              <span className="tb-btn" title={info?.maximized ? '还原' : '最大化'} onClick={() => act('toggle_maximize')}>
                {info?.maximized ? <IconCopy size={14} /> : <IconSquare size={14} />}
              </span>
              <span className="tb-btn close" title="关闭" onClick={() => act('close')}>
                <IconX size={16} />
              </span>
            </>
          )}
        </div>
      )}
    </div>
  )
}
