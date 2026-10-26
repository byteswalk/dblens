// 主窗口布局（M1 设计稿第 1 部分）：标题栏；活动栏 + 可拖动、可折叠的侧栏 + 工作区；状态栏。
// 侧栏宽度经 useDefaultLayout 存进设置（ui.layout.*），下次启动恢复。

import { Button, Empty } from 'antd'
import { IconDatabase, IconHistory, IconPlus, IconSettings, IconStar, IconTool } from '@tabler/icons-react'
import { useEffect, useRef } from 'react'
import { Group, Panel, type PanelImperativeHandle, Separator, useDefaultLayout } from 'react-resizable-panels'

import ObjectTree from '../features/tree/ObjectTree'
import { layoutStorage } from '../shared/settings'
import StatusBar from './StatusBar'
import TitleBar from './TitleBar'
import { type SideKey, useUi } from './ui'

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
  return (
    <div className="welcome">
      <IconDatabase size={48} stroke={1.2} />
      <div style={{ fontSize: 16, color: 'var(--text)' }}>DBLens 数洞</div>
      <Button type="primary" icon={<IconPlus size={14} />} onClick={() => openConnection(null)}>
        新建连接
      </Button>
      <div className="muted">在左侧的数据库树中展开连接，浏览库表结构</div>
    </div>
  )
}

export default function Shell() {
  const { side, toggleSide, setSettingsOpen, openConnection } = useUi()
  const sidePanel = useRef<PanelImperativeHandle | null>(null)
  const layout = useDefaultLayout({ id: 'main', storage: layoutStorage })

  // 侧栏折叠状态跟随 side
  useEffect(() => {
    const p = sidePanel.current
    if (!p) return
    if (side === null) p.collapse()
    else if (p.isCollapsed()) p.expand()
  }, [side])

  // Ctrl+N 新建连接
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if ((e.ctrlKey || e.metaKey) && !e.shiftKey && !e.altKey && e.key.toLowerCase() === 'n') {
        e.preventDefault()
        openConnection(null)
      }
    }
    window.addEventListener('keydown', onKey)
    return () => window.removeEventListener('keydown', onKey)
  }, [openConnection])

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
            <Welcome />
          </Panel>
        </Group>
      </div>
      <StatusBar />
    </div>
  )
}
