// 工作区（设计稿第 2 部分）：标签栏和当前标签页。标签顶部有一条环境色线，生产连接的标签整块标红、
// 标题带"生产"。懒加载：Monaco 和结果表格只在打开第一个查询标签时才加载。

import { IconFileCode, IconPlus, IconX } from '@tabler/icons-react'
import { useQuery } from '@tanstack/react-query'
import { useEffect, useRef } from 'react'

import { useApi } from '../../app/context'
import QueryView from './QueryView'
import { disposeModel } from './SqlEditor'
import { useQueryActions } from './actions'
import { useTabRuntime } from './runner'
import { type QueryTab, useTabs } from './tabs'

function TabTitle({ tab, active, envClass, profileName }: {
  tab: QueryTab
  active: boolean
  envClass: string
  profileName: string
}) {
  const rt = useTabRuntime(tab.id)
  const { closeQuery } = useQueryActions()
  return (
    <div
      className={`tab ${envClass}${active ? ' on' : ''}`}
      onClick={() => useTabs.getState().activate(tab.id)}
      onMouseDown={(e) => {
        // 中键关闭
        if (e.button === 1) {
          e.preventDefault()
          closeQuery(tab.id)
        }
      }}
      title={`${tab.title} · ${profileName}`}
    >
      <IconFileCode size={14} />
      <span className="tab-title">{tab.title}</span>
      {envClass === 'prod' ? <span>· 生产</span> : <span className="muted">{profileName}</span>}
      {rt.running && <span className="running-dot" />}
      <span
        className="x"
        onClick={(e) => {
          e.stopPropagation()
          closeQuery(tab.id)
        }}
      >
        <IconX size={12} />
      </span>
    </div>
  )
}

export default function Workspace() {
  const api = useApi()
  const { tabs, active } = useTabs()
  const { newQuery } = useQueryActions()
  const { data: profiles = [] } = useQuery({ queryKey: ['profiles'], queryFn: () => api.profiles() })

  // 关闭的标签页：删除它的编辑器模型
  const known = useRef(new Set<string>())
  useEffect(() => {
    const now = new Set(tabs.map((t) => t.id))
    for (const id of known.current) if (!now.has(id)) disposeModel(id)
    known.current = now
  }, [tabs])

  const tab = tabs.find((t) => t.id === active)
  return (
    <div className="workspace">
      <div className="row tabs">
        {tabs.map((t) => {
          const p = profiles.find((x) => x.id === t.profileId)
          return (
            <TabTitle
              key={t.id}
              tab={t}
              active={t.id === active}
              envClass={p?.environment && p.environment !== 'none' ? p.environment : ''}
              profileName={p?.name ?? '连接已删除'}
            />
          )
        })}
        <div className="tab add" title="新建查询（Ctrl+T）" onClick={() => newQuery()}>
          <IconPlus size={14} />
        </div>
      </div>
      {tab && <QueryView tab={tab} profiles={profiles} />}
    </div>
  )
}
