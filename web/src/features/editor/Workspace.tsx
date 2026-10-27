// 工作区（设计稿第 2 部分）：标签栏和当前标签页（查询标签或数据标签）。标签顶部有一条环境色线，生产连接的标签整块标红、
// 标题带"生产"。懒加载：Monaco 和结果表格只在打开第一个查询标签时才加载。

import { IconFileCode, IconPlus, IconTable, IconX } from '@tabler/icons-react'
import { useQuery } from '@tanstack/react-query'
import { useEffect, useRef } from 'react'
import { useTranslation } from 'react-i18next'

import { useApi } from '../../app/context'
import DbIcon from '../../shared/DbIcon'
import type { Profile } from '../../shared/api'
import DataView from '../data/DataView'
import QueryView from './QueryView'
import { disposeModel } from './SqlEditor'
import { useQueryActions } from './actions'
import { useTabRuntime } from './runner'
import { type QueryTab, useTabs } from './tabs'

function TabTitle({ tab, active, envClass, profileName, profile }: {
  tab: QueryTab
  active: boolean
  envClass: string
  profileName: string
  profile: Profile | undefined
}) {
  const { t } = useTranslation()
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
      {tab.data ? (
        <IconTable size={16} />
      ) : profile ? (
        <DbIcon driver={profile.driver} variant={rt.session?.variant ?? profile.variant} />
      ) : (
        <IconFileCode size={16} />
      )}
      <span className="tab-title">{tab.title}</span>
      {envClass === 'prod' ? <span>· {t('editor.tab.prod')}</span> : <span className="muted">{profileName}</span>}
      {rt.running && <span className="running-dot" />}
      <span
        className="x"
        onClick={(e) => {
          e.stopPropagation()
          closeQuery(tab.id)
        }}
      >
        <IconX size={14} />
      </span>
    </div>
  )
}

export default function Workspace() {
  const api = useApi()
  const { t } = useTranslation()
  const { tabs, active } = useTabs()
  const { newQuery } = useQueryActions()
  const { data: profiles = [] } = useQuery({ queryKey: ['profiles'], queryFn: () => api.profiles() })

  // 关闭的标签页：删除它的编辑器模型
  const known = useRef(new Set<string>())
  useEffect(() => {
    const now = new Set(tabs.map((x) => x.id))
    for (const id of known.current) if (!now.has(id)) disposeModel(id)
    known.current = now
  }, [tabs])

  const tab = tabs.find((x) => x.id === active)
  return (
    <div className="workspace">
      <div className="row tabs">
        {tabs.map((tb) => {
          const p = profiles.find((x) => x.id === tb.profileId)
          return (
            <TabTitle
              key={tb.id}
              tab={tb}
              active={tb.id === active}
              envClass={p?.environment && p.environment !== 'none' ? p.environment : ''}
              profileName={p?.name ?? t('editor.tab.profileDeleted')}
              profile={p}
            />
          )
        })}
        <div className="tab add" title={t('editor.tab.newQuery')} onClick={() => newQuery()}>
          <IconPlus size={16} />
        </div>
      </div>
      {tab &&
        (tab.data ? <DataView key={tab.id} tab={tab} profiles={profiles} /> : <QueryView tab={tab} profiles={profiles} />)}
    </div>
  )
}
