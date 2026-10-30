// 自绘标题栏（M1 设计稿第 1、5 部分）：菜单、居中标题、置顶按钮、窗口按钮。
// 空白处按下鼠标开始拖动窗口，双击最大化或还原。macOS 不画菜单（用系统菜单栏），左侧给红黄绿按钮留位置；
// 浏览器模式只有菜单和标题。

import { App, Dropdown, type MenuProps } from 'antd'
import { useQuery } from '@tanstack/react-query'
import { IconCheck, IconCopy, IconMinus, IconPin, IconSquare, IconX } from '@tabler/icons-react'
import { type MouseEvent, useEffect } from 'react'
import { useTranslation } from 'react-i18next'

import { useQueryActions } from '../features/editor/actions'
import { type EditorCommand, sendCommand } from '../features/editor/commands'
import { useTabs } from '../features/editor/tabs'
import { useImportDialogs } from '../features/import/dialog'
import { useModelDialogs } from '../features/model/dialogs'
import { useSetting, useSettings } from '../shared/settings'
import { type WindowAction, desktop, titleBarInset } from '../shared/shell'
import { THEME_KEY, type ThemePref, parseThemePref } from '../shared/theme'
import { type MenuGroup, buildMenus } from './menus'
import { useApi } from './context'
import logo from './logo.svg'
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
  const { t } = useTranslation()
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
  const appName = t('common.appName')
  const title = tab
    ? t('app.titleBar.windowTitle', {
        tab: tab.title,
        // 模型的设计器标签、比对标签不绑定连接
        connection: tab.profileId ? (profile?.name ?? t('app.status.connectionDeleted')) : t('app.side.models'),
        app: appName,
      })
    : appName
  useEffect(() => {
    document.title = title
  }, [title])

  const run = (key: string) => {
    const [kind, arg] = key.split(':')
    if (key === 'new-connection') ui.openConnection(null)
    else if (key === 'new-query') newQuery()
    else if (key === 'close-tab') closeActive()
    else if (key === 'new-table') {
      // 连接同"导入"；schema 用当前数据标签或设计器标签的，没有时在设计器的"选项"中选
      const id = ui.selectedProfile ?? (tab?.profileId || undefined) ?? profiles?.[0]?.id
      const schema = (tab?.profileId === id && (tab?.data?.schema ?? tab?.design?.schema)) || ''
      if (!id) ui.openConnection(null)
      else useTabs.getState().openDesign(id, { schema, table: null })
    }
    else if (key === 'import' || key === 'sql-file') {
      // 用对象树中选中的连接，没有时用当前标签的、第一个连接；一个都没有时新建连接
      const id = ui.selectedProfile ?? (tab?.profileId || undefined) ?? profiles?.[0]?.id
      if (!id) ui.openConnection(null)
      else if (key === 'import') useImportDialogs.getState().openImport({ profileId: id, schema: null, table: null, create: false })
      else useImportDialogs.getState().openSqlFile({ profileId: id, schema: null })
    }
    else if (key === 'open-model') useModelDialogs.getState().openOpen()
    else if (key === 'new-model') useModelDialogs.getState().openCreate()
    else if (key === 'compare') useTabs.getState().openCompare({ source: null, target: null })
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
      else modal.info({ title: appName, content: <a href={RELEASES}>{RELEASES}</a> })
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

  // 只有空白处（不是菜单和按钮）才拖动窗口。下拉菜单挂在 body 下，但事件会沿 React 树冒泡到这里，要排除
  const onMouseDown = (e: MouseEvent<HTMLDivElement>) => {
    const target = e.target as HTMLElement
    if (!desktop || e.button !== 0 || !e.currentTarget.contains(target)) return
    if (target.closest('[data-no-drag]')) return
    act(e.detail === 2 ? 'toggle_maximize' : 'start_dragging')
  }

  return (
    <div className="row titlebar" onMouseDown={onMouseDown} style={{ paddingLeft: titleBarInset(info?.platform) }}>
      <img src={logo} width={16} height={16} alt="" className="logo" />
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
            title={t('app.titleBar.pin')}
            onClick={() => act('toggle_always_on_top')}
          >
            <IconPin size={16} />
          </span>
          {!mac && (
            <>
              <span className="tb-btn" title={t('app.titleBar.minimize')} onClick={() => act('minimize')}>
                <IconMinus size={16} />
              </span>
              <span className="tb-btn" title={info?.maximized ? t('app.titleBar.restore') : t('app.titleBar.maximize')} onClick={() => act('toggle_maximize')}>
                {info?.maximized ? <IconCopy size={14} /> : <IconSquare size={14} />}
              </span>
              <span className="tb-btn close" title={t('app.titleBar.close')} onClick={() => act('close')}>
                <IconX size={16} />
              </span>
            </>
          )}
        </div>
      )}
    </div>
  )
}
