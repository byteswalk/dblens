// 两次检查的对比：结论变化、新增问题、已解决的问题。

import { useQuery } from '@tanstack/react-query'
import { Empty, Spin, Table } from 'antd'
import { useTranslation } from 'react-i18next'

import { useApi } from '../../app/context'
import type { LintRun, LintVerdict } from '../../shared/api'
import type { QueryTab } from '../editor/tabs'
import { type DiffIssue, type DiffVerdict, diffRuns, formatTime, summarize } from './logic'

export default function DiffView({ tab }: { tab: QueryTab }) {
  const api = useApi()
  const { t } = useTranslation()
  const target = tab.lint?.view === 'diff' ? tab.lint : null
  const before = useQuery({
    queryKey: ['lint', 'run', target?.before],
    queryFn: () => api.lintRun(target?.before ?? ''),
    enabled: !!target,
  })
  const after = useQuery({
    queryKey: ['lint', 'run', target?.after],
    queryFn: () => api.lintRun(target?.after ?? ''),
    enabled: !!target,
  })

  if (before.isLoading || after.isLoading) return <Spin style={{ margin: 48 }} />
  if (!before.data || !after.data) return <Empty style={{ marginTop: 48 }} description={t('lint.run.missing')} />

  const d = diffRuns(before.data, after.data)
  const verdict = (v?: LintVerdict) => (v ? t(`lint.verdict.${v}`) : t('lint.diff.none'))
  const issues = (rows: DiffIssue[]) => (
    <Table<DiffIssue>
      size="small"
      pagination={false}
      rowKey={(_, i) => String(i)}
      dataSource={rows}
      columns={[
        { title: t('lint.report.clause'), dataIndex: 'text' },
        { title: t('lint.report.object_col'), width: 220, render: (_, r) => <code>{r.issue.object}</code> },
        { title: t('lint.report.message'), render: (_, r) => r.issue.message },
      ]}
    />
  )
  const heading = (text: string, n: number) => (
    <div style={{ fontWeight: 600, margin: '14px 0 4px' }}>
      {text}（{n}）
    </div>
  )
  const side = (label: string, run: LintRun) => {
    const s = summarize(run.results)
    return (
      <div>
        <div style={{ color: '#888', fontSize: 12 }}>{label}</div>
        <div style={{ fontWeight: 600 }}>{run.target}</div>
        <div style={{ color: '#888', fontSize: 12 }}>
          {run.ruleset_name} · {formatTime(run.created_at)}
        </div>
        <div>
          {t('lint.report.pass_rate')} {s.passRate === null ? '-' : `${s.passRate}%`} · {t('lint.report.issues')} {s.issues}
        </div>
      </div>
    )
  }

  return (
    <div style={{ padding: '12px 16px', overflow: 'auto', flex: 1, minHeight: 0 }}>
      <div style={{ display: 'flex', gap: 48, flexWrap: 'wrap' }}>
        {side(t('lint.diff.before'), before.data)}
        {side(t('lint.diff.after'), after.data)}
      </div>
      {heading(t('lint.diff.verdicts'), d.verdicts.length)}
      {d.verdicts.length > 0 && (
        <Table<DiffVerdict>
          size="small"
          pagination={false}
          rowKey="clause"
          dataSource={d.verdicts}
          columns={[
            { title: t('lint.report.clause'), dataIndex: 'text' },
            { title: t('lint.diff.before'), width: 130, render: (_, r) => verdict(r.before) },
            { title: t('lint.diff.after'), width: 130, render: (_, r) => verdict(r.after) },
          ]}
        />
      )}
      {heading(t('lint.diff.added'), d.added.length)}
      {d.added.length > 0 && issues(d.added)}
      {heading(t('lint.diff.resolved'), d.resolved.length)}
      {d.resolved.length > 0 && issues(d.resolved)}
    </div>
  )
}
