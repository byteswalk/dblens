// 数据标签（M1c2 设计文档第 5 节）：只读地浏览一张表 / 视图 / 物化视图。
// 工具栏：连接和 schema（只显示）、WHERE 过滤、刷新、停止、统计行数；表格沿用查询标签的结果面板，点列头排序。
// SELECT 由后端按方言生成（data-sql），经这个标签自己的会话执行（沿用 runner：续取、取消、行数上限）。

import { IconDownload, IconPlayerStop, IconRefresh, IconSum } from '@tabler/icons-react'
import { Alert, App, Button, Input, Tooltip } from 'antd'
import { useEffect, useState } from 'react'
import { useTranslation } from 'react-i18next'

import { useApi } from '../../app/context'
import DbIcon from '../../shared/DbIcon'
import type { Profile } from '../../shared/api'
import ResultsPanel from '../results/ResultsPanel'
import { runnerFor, useTabRuntime } from '../editor/runner'
import { useExportDialog } from '../export/dialog'
import { type DataTarget, type QueryTab, useTabs } from '../editor/tabs'
import { countRows } from './count'
import { nextOrder } from './sort'

export default function DataView({ tab, profiles }: { tab: QueryTab; profiles: Profile[] }) {
  const api = useApi()
  const { t } = useTranslation()
  const { message } = App.useApp()
  const runner = runnerFor(api, tab.id)
  const rt = useTabRuntime(tab.id)
  const profile = profiles.find((p) => p.id === tab.profileId)
  const data = tab.data!
  const [filter, setFilter] = useState(data.filter ?? '')
  const [total, setTotal] = useState<string | null>(null)
  const [counting, setCounting] = useState(false)

  const load = async (d: DataTarget) => {
    try {
      const sql = await api.dataSql(tab.profileId, d.schema, { object: d.object, filter: d.filter, order: d.order })
      await runner.rerun(sql)
    } catch (e) {
      message.error((e as Error).message)
    }
  }

  /** 改过滤条件或排序：保存到标签，重新读取。 */
  const apply = (patch: Pick<DataTarget, 'filter' | 'order'>) => {
    const next = { ...data, ...patch }
    useTabs.getState().setData(tab.id, patch)
    if ('filter' in patch) setTotal(null)
    void load(next)
  }

  // 第一次显示时（以及连接从已删除恢复时）读取；切换回来时保留已读取的结果
  useEffect(() => {
    if (profile && !rt.submission && !rt.running) void load(data)
  }, [profile?.id])

  const count = async () => {
    setCounting(true)
    try {
      const sql = await api.dataSql(tab.profileId, data.schema, { object: data.object, filter: data.filter, count: true })
      setTotal(await countRows(api, tab.profileId, sql))
    } catch (e) {
      message.error((e as Error).message)
    } finally {
      setCounting(false)
    }
  }

  const loaded = rt.exec.statements[0]?.results[0]?.rows.length ?? 0
  return (
    <div className="query">
      <div className="row toolbar">
        {profile && <DbIcon driver={profile.driver} variant={rt.session?.variant ?? profile.variant} />}
        <span>{profile?.name}</span>
        <span className="muted">{data.schema}</span>
        <span className="sep" />
        <Input
          size="small"
          allowClear
          style={{ flex: 1, fontFamily: 'var(--mono)' }}
          placeholder={t('editor.data.filterPlaceholder')}
          value={filter}
          disabled={!profile}
          onChange={(e) => {
            setFilter(e.target.value)
            // 清空即取消过滤
            if (!e.target.value && data.filter) apply({ filter: undefined })
          }}
          onPressEnter={() => apply({ filter: filter.trim() || undefined })}
        />
        <Tooltip title={t('editor.data.refresh')} mouseEnterDelay={0.6}>
          <Button type="text" size="small" disabled={!profile} onClick={() => void load(data)}>
            <IconRefresh size={16} />
          </Button>
        </Tooltip>
        <Tooltip title={t('editor.data.stop')} mouseEnterDelay={0.6}>
          <Button type="text" size="small" disabled={!rt.running} onClick={() => runner.cancel()}>
            <IconPlayerStop size={16} />
          </Button>
        </Tooltip>
        <Button size="small" icon={<IconSum size={14} />} loading={counting} disabled={!profile} onClick={() => void count()}>
          {t('editor.data.count')}
        </Button>
        <Button
          size="small"
          icon={<IconDownload size={14} />}
          disabled={!profile}
          onClick={() =>
            useExportDialog.getState().open({
              profileId: tab.profileId,
              source: { data: { schema: data.schema, object: data.object, filter: data.filter, order: data.order } },
              baseName: data.object,
              table: `${data.schema}.${data.object}`,
              inTransaction: false,
            })
          }
        >
          {t('results.export.button')}
        </Button>
        {total !== null && <span className="muted">{t('editor.data.total', { count: Number(total) })}</span>}
      </div>
      {!profile && <Alert type="error" banner message={t('editor.query.profileDeleted')} />}
      <div className="grow" style={{ minHeight: 0 }}>
        <ResultsPanel
          rt={rt}
          onFetchMore={() => runner.fetchMore()}
          sort={data.order ?? null}
          onSort={(column) => apply({ order: nextOrder(data.order, column) })}
        />
      </div>
      <div className="row muted" style={{ padding: '2px 8px', gap: 8, fontSize: 'var(--fs-sm)', borderTop: '1px solid var(--border)' }}>
        <span>{t('editor.data.loaded', { count: loaded })}</span>
        <span className="grow" />
        <span>{t('editor.data.sortHint')}</span>
      </div>
    </div>
  )
}
