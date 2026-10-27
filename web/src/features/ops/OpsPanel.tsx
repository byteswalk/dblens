// 运维查询侧栏（M1c3 设计文档 5.3 节）：按当前连接的方言列出会话、锁、存储、SQL 统计等查询。
// 点击用该连接新开标签并执行；右键在新标签中打开（不执行）、复制 SQL。结束会话的语句在结果的 kill_sql 列，只生成不执行。

import { App, Button, Dropdown, Empty, Select } from 'antd'
import { IconPlayerPlay } from '@tabler/icons-react'
import { useQuery } from '@tanstack/react-query'
import { useState } from 'react'
import { useTranslation } from 'react-i18next'

import { useApi } from '../../app/context'
import { useUi } from '../../app/ui'
import DbIcon from '../../shared/DbIcon'
import type { OpsQuery } from '../../shared/api'
import { useQueryActions } from '../editor/actions'
import { useTabs } from '../editor/tabs'
import { groupByCategory, pickProfile } from './ops'

export default function OpsPanel() {
  const api = useApi()
  const { t, i18n } = useTranslation()
  const { message } = App.useApp()
  const { openSql } = useQueryActions()
  const { data: profiles = [] } = useQuery({ queryKey: ['profiles'], queryFn: () => api.profiles() })
  const selected = useUi((s) => s.selectedProfile)
  const openConnection = useUi((s) => s.openConnection)
  const activeProfile = useTabs((s) => s.tabs.find((x) => x.id === s.active)?.profileId)
  const [chosen, setChosen] = useState<string | null>(null)
  const profileId = pickProfile(profiles, chosen, selected, activeProfile)
  const { data = [], error } = useQuery({
    queryKey: ['ops', profileId, i18n.language],
    queryFn: () => api.ops(profileId!),
    enabled: profileId !== null,
  })

  if (!profileId) {
    return (
      <Empty style={{ marginTop: 48 }} description={t('app.ops.noProfile')} image={Empty.PRESENTED_IMAGE_SIMPLE}>
        <Button type="primary" onClick={() => openConnection(null)}>
          {t('app.ops.newConnection')}
        </Button>
      </Empty>
    )
  }

  const run = (q: OpsQuery, execute: boolean) => openSql(q.sql, profileId, { run: execute })
  return (
    <div className="hist">
      <div className="hist-filter">
        <Select
          size="small"
          value={profileId}
          onChange={setChosen}
          options={profiles.map((p) => ({
            value: p.id,
            label: (
              <span>
                <DbIcon driver={p.driver} variant={p.variant} /> {p.name}
              </span>
            ),
          }))}
        />
        <div className="muted" style={{ fontSize: 'var(--fs-xs)' }}>
          {t('app.ops.hint')}
        </div>
      </div>
      <div className="hist-list">
        {error && <div className="err" style={{ padding: 12 }}>{(error as Error).message}</div>}
        {groupByCategory(data).map((g) => (
          <div key={g.category}>
            <div className="hist-day">{t(`app.ops.category.${g.category}`)}</div>
            {g.items.map((q) => (
              <Dropdown
                key={q.key}
                trigger={['contextMenu']}
                menu={{
                  items: [
                    { key: 'open', label: t('app.ops.openNoRun') },
                    { key: 'copy', label: t('app.ops.copy') },
                  ],
                  onClick: ({ key }) => {
                    if (key === 'open') run(q, false)
                    else
                      void navigator.clipboard.writeText(q.sql).then(() => message.success(t('app.ops.copied')))
                  },
                }}
              >
                <div className="hist-item row" style={{ gap: 8, cursor: 'pointer' }} onClick={() => run(q, true)} title={q.sql}>
                  <IconPlayerPlay size={14} className="muted" />
                  <span>{q.name}</span>
                </div>
              </Dropdown>
            ))}
          </div>
        ))}
      </div>
    </div>
  )
}
