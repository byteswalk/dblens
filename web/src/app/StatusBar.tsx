// 状态栏（M1 设计文档 1.1 节）：有查询标签时显示它的连接和方言、事务状态、最近一次执行的行数和耗时、
// 光标位置；没有时显示对象树中选中的连接。

import { IconPlugConnected } from '@tabler/icons-react'
import { useQuery } from '@tanstack/react-query'
import { useTranslation } from 'react-i18next'

import { driverLabel } from '../features/connections/form'
import { useCursor } from '../features/editor/cursor'
import { variantName } from '../features/editor/dialect'
import { useTabRuntime } from '../features/editor/runner'
import { useTabs } from '../features/editor/tabs'
import { lastSummary } from '../features/results/results'
import type { Profile } from '../shared/api'
import { useApi } from './context'
import { useUi } from './ui'

function TabStatus({ tabId, profile }: { tabId: string; profile: Profile | undefined }) {
  const { t } = useTranslation()
  const rt = useTabRuntime(tabId)
  const cursor = useCursor()
  const s = rt.session
  const dialect = variantName(s?.variant) ?? (profile && driverLabel(profile.driver))
  const summary = lastSummary(rt.exec)
  return (
    <>
      <span className="row" style={{ gap: 4 }}>
        <IconPlugConnected size={14} />
        {profile?.name ?? t('app.status.connectionDeleted')}
        {s?.schema && <span className="muted">· {s.schema}</span>}
      </span>
      {s && (
        <span style={{ color: s.in_transaction ? 'var(--warn)' : undefined }}>
          {s.autocommit
            ? t('app.status.autocommit')
            : s.in_transaction
              ? t('app.status.inTransaction')
              : t('app.status.manualCommit')}
        </span>
      )}
      {rt.running ? <span>{t('app.status.running')}</span> : summary && <span>{summary}</span>}
      <span className="grow" />
      <span>{t('app.status.cursor', { line: cursor.line, column: cursor.column })}</span>
      {dialect && <span>{dialect}</span>}
      <span>UTF-8</span>
    </>
  )
}

export default function StatusBar() {
  const { t } = useTranslation()
  const api = useApi()
  const selected = useUi((s) => s.selectedProfile)
  const active = useTabs((s) => s.tabs.find((t) => t.id === s.active))
  const { data: profiles } = useQuery({ queryKey: ['profiles'], queryFn: () => api.profiles() })
  if (active) {
    return (
      <div className="row statusbar">
        <TabStatus tabId={active.id} profile={profiles?.find((x) => x.id === active.profileId)} />
      </div>
    )
  }
  const p = profiles?.find((x) => x.id === selected)
  return (
    <div className="row statusbar">
      {p ? (
        <span className="row" style={{ gap: 4 }}>
          <IconPlugConnected size={14} />
          {p.name} · {driverLabel(p.driver)} · {p.user}@{p.host}:{p.port}
        </span>
      ) : (
        <span className="muted">{t('app.status.noConnection')}</span>
      )}
      <span className="grow" />
    </div>
  )
}
