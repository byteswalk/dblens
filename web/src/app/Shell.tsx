// 主窗口布局（M1 设计稿第 1 部分）：标题栏；活动栏 + 可拖动、可折叠的侧栏 + 工作区；状态栏。
// 侧栏宽度经 useDefaultLayout 存进设置（ui.layout.*），下次启动恢复。
// 没有查询标签时工作区显示欢迎页；工作区（Monaco、结果表格）懒加载。

import { Button, Empty, Spin } from 'antd'
import { IconDatabase, IconFileCode, IconHistory, IconPlus, IconSettings, IconStar, IconTool } from '@tabler/icons-react'
import { Suspense, lazy, useEffect, useRef } from 'react'
import { Group, Panel, type PanelImperativeHandle, Separator, useDefaultLayout } from 'react-resizable-panels'

import { useQueryActions } from '../features/editor/actions'
import { type EditorCommand, sendCommand } from '../features/editor/commands'
import { useTabs } from '../features/editor/tabs'
import ObjectTree from '../features/tree/ObjectTree'
import { layoutStorage } from '../shared/settings'
import StatusBar from './StatusBar'
import TitleBar from './TitleBar'
import { type SideKey, useUi } from './ui'

// Monaco 自带文字的语言要在它加载前设定，所以先加载语言包再加载工作区
const Workspace = lazy(async () => {
  const { loadMonacoLocale } = await import('../features/editor/monacoLocale')
  await loadMonacoLocale()
  return import('../features/editor/Workspace')
})

const SIDES: { key: SideKey; title: string; icon: typeof IconDatabase }[] = [
  { key: 'db', title: '数据库', icon: IconDatabase },
  { key: 'history', title: '历史', icon: IconHistory },
  { key: 'favorites', title: '收藏', icon: IconStar },
  { key: 'ops', title: '运维查询', icon: IconTool },
]

function SideContent({ side }: { side: SideKey }) {
  if (side === 'db') return <ObjectTree />
  return <Empty style={{ marginTop: 48 }} description="即将推出" image={Empty.PRESENTED_IMAGE_SIMPLE} />
}

function Welcome() {
  const openConnection = useUi((s) => s.openConnection)
  const { newQuery } = useQueryActions()
  return (
    <div className="welcome">
      <IconDatabase size={48} stroke={1.2} />
      <div style={{ fontSize: 16, color: 'var(--text)' }}>DBLens 数洞</div>
      <div className="row" style={{ gap: 8 }}>
        <Button type="primary" icon={<IconPlus size={14} />} onClick={() => openConnection(null)}>
          新建连接
        </Button>
        <Button icon={<IconFileCode size={14} />} onClick={() => newQuery()}>
          新建查询
        </Button>
      </div>
      <div className="muted">在左侧的数据库树中展开连接浏览库表结构，右键连接可新建查询（Ctrl+T）</div>
    </div>
  )
}

/** 全局快捷键（M1 设计文档 1.3 节）。焦点在编辑器里时由 Monaco 的动作处理，这里处理焦点在别处的情况。 */
function shortcut(e: KeyboardEvent): EditorCommand | 'new-connection' | 'new-query' | 'close-tab' | null {
  const ctrl = e.ctrlKey || e.metaKey
  const key = e.key.toLowerCase()
  if (ctrl && !e.shiftKey && !e.altKey && key === 'n') return 'new-connection'
  if (ctrl && !e.shiftKey && !e.altKey && key === 't') return 'new-query'
  if (ctrl && !e.shiftKey && !e.altKey && key === 'w') return 'close-tab'
  if (ctrl && key === 'enter') return 'run-current'
  if (!ctrl && !e.altKey && key === 'f5') return 'run-all'
  if ((ctrl && key === 'pause') || (e.metaKey && key === '.')) return 'cancel'
  if (e.shiftKey && e.altKey && key === 'f') return 'format'
  return null
}

export default function Shell() {
  const { side, toggleSide, setSettingsOpen, openConnection } = useUi()
  const { newQuery, closeActive } = useQueryActions()
  const hasTabs = useTabs((s) => s.tabs.length > 0)
  const sidePanel = useRef<PanelImperativeHandle | null>(null)
  const layout = useDefaultLayout({ id: 'main', storage: layoutStorage })

  // 侧栏折叠状态跟随 side
  useEffect(() => {
    const p = sidePanel.current
    if (!p) return
    if (side === null) p.collapse()
    else if (p.isCollapsed()) p.expand()
  }, [side])

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      const cmd = shortcut(e)
      if (!cmd) return
      e.preventDefault()
      if (cmd === 'new-connection') openConnection(null)
      else if (cmd === 'new-query') newQuery()
      else if (cmd === 'close-tab') closeActive()
      else sendCommand(cmd)
    }
    window.addEventListener('keydown', onKey)
    return () => window.removeEventListener('keydown', onKey)
  }, [openConnection, newQuery, closeActive])

  const current = SIDES.find((s) => s.key === side)
  return (
    <div className="shell">
      <TitleBar />
      <div className="row grow" style={{ alignItems: 'stretch', minHeight: 0 }}>
        <div className="activity">
          {SIDES.map(({ key, title, icon: Icon }) => (
            <span key={key} className={`a${side === key ? ' on' : ''}`} title={title} onClick={() => toggleSide(key)}>
              <Icon size={20} />
            </span>
          ))}
          <span className="grow" />
          <span className="a" title="设置" onClick={() => setSettingsOpen(true)}>
            <IconSettings size={20} />
          </span>
        </div>
        <Group orientation="horizontal" className="grow" {...layout}>
          <Panel id="side" panelRef={sidePanel} defaultSize={270} minSize={200} maxSize={600} collapsible>
            {current && (
              <div className="side">
                <div className="row side-h">{current.title}</div>
                <div className="side-body">
                  <SideContent side={current.key} />
                </div>
              </div>
            )}
          </Panel>
          <Separator className="splitter" />
          <Panel id="work" minSize={400}>
            {hasTabs ? (
              <Suspense fallback={<Spin style={{ margin: 48 }} />}>
                <Workspace />
              </Suspense>
            ) : (
              <Welcome />
            )}
          </Panel>
        </Group>
      </div>
      <StatusBar />
    </div>
  )
}
