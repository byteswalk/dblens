// 检查结果：总分统计、按分类分组的条款、问题明细、人工结论、导出报告。

import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { App, Button, Empty, Input, Select, Space, Spin, Table, Tag } from 'antd'
import { useState } from 'react'
import { useTranslation } from 'react-i18next'

import { useApi } from '../../app/context'
import type { LintClauseResult, LintIssue, LintVerdict } from '../../shared/api'
import type { QueryTab } from '../editor/tabs'
import { effectiveVerdict, groupBySection, reviewable, summarize, verdictTone } from './logic'

const TONE: Record<string, string> = { ok: '#2a7d4f', bad: '#c0392b', warn: '#b8860b', muted: '#999' }

type Row = ReturnType<typeof groupBySection>[number]['rows'][number]

function VerdictTag({ v }: { v: LintVerdict }) {
  const { t } = useTranslation()
  return <span style={{ color: TONE[verdictTone(v)], fontWeight: v === 'fail' ? 600 : 400 }}>{t(`lint.verdict.${v}`)}</span>
}

export default function RunView({ tab }: { tab: QueryTab }) {
  const api = useApi()
  const { t } = useTranslation()
  const { message } = App.useApp()
  const qc = useQueryClient()
  const runId = tab.lint?.view === 'run' ? tab.lint.run : ''
  const [reviewer, setReviewer] = useState('')

  const key = ['lint', 'run', runId]
  const { data: run, isLoading, error } = useQuery({ queryKey: key, queryFn: () => api.lintRun(runId), enabled: !!runId })

  const review = useMutation({
    mutationFn: ({ clause, verdict, note }: { clause: string; verdict: LintVerdict; note: string }) =>
      api.reviewLintClause(runId, clause, { verdict, note }),
    onSuccess: (updated) => qc.setQueryData(key, updated),
    onError: (e) => message.error(e instanceof Error ? e.message : String(e)),
  })

  if (isLoading) return <Spin style={{ margin: 48 }} />
  if (error || !run)
    return <Empty style={{ marginTop: 48 }} description={error instanceof Error ? error.message : t('lint.run.missing')} />

  const s = summarize(run.results)
  const tiles: [LintVerdict | 'issues', number][] = [
    ['pass', s.pass],
    ['fail', s.fail],
    ['unknown', s.unknown],
    ['manual', s.manual],
    ['not_applicable', s.not_applicable],
    ['issues', s.issues],
  ]

  return (
    <div style={{ padding: '12px 16px', overflow: 'auto', height: '100%' }}>
      <Space direction="vertical" size="small" style={{ width: '100%' }}>
        <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', flexWrap: 'wrap', gap: 12 }}>
          <div>
            <div style={{ fontSize: 18, fontWeight: 600 }}>{run.target}</div>
            <div style={{ color: '#888', fontSize: 12 }}>{run.ruleset_name}</div>
          </div>
          <Space>
            <Input
              size="small"
              style={{ width: 140 }}
              placeholder={t('lint.report.reviewer')}
              value={reviewer}
              onChange={(e) => setReviewer(e.target.value)}
            />
            <Button
              size="small"
              onClick={() => window.open(api.lintReportUrl(runId, 'html', reviewer || undefined), '_blank')}
            >
              {t('lint.run.report')}
            </Button>
          </Space>
        </div>

        <div style={{ display: 'flex', gap: 8, flexWrap: 'wrap', alignItems: 'baseline' }}>
          {s.passRate !== null && (
            <div style={{ fontSize: 22, fontWeight: 600, marginRight: 8 }}>
              {t('lint.report.pass_rate')} {s.passRate}%
            </div>
          )}
          {tiles.map(([k, n]) => (
            <Tag key={k} style={{ fontSize: 13, padding: '2px 8px' }}>
              {k === 'issues' ? t('lint.report.issues') : t(`lint.verdict.${k}`)}: <b>{n}</b>
            </Tag>
          ))}
        </div>

        {groupBySection(run).map((g) => (
          <Section
            key={g.section}
            title={t(`lint.section.${g.section}`)}
            rows={g.rows}
            onReview={(clause, verdict, note) => review.mutate({ clause, verdict, note })}
            saving={review.isPending}
          />
        ))}
      </Space>
    </div>
  )
}

function Section({
  title,
  rows,
  onReview,
  saving,
}: {
  title: string
  rows: Row[]
  onReview: (clause: string, verdict: LintVerdict, note: string) => void
  saving: boolean
}) {
  const { t } = useTranslation()
  return (
    <div>
      <div style={{ fontWeight: 600, margin: '10px 0 4px' }}>{title}</div>
      <Table<Row>
        size="small"
        pagination={false}
        rowKey={(r) => r.result.clause}
        dataSource={rows}
        expandable={{
          rowExpandable: (r) => r.result.issues.length > 0,
          expandedRowRender: (r) => <Issues result={r.result} />,
        }}
        columns={[
          { title: t('lint.report.clause'), render: (_, r) => r.meta?.text ?? r.result.clause },
          {
            title: t('lint.report.verdict'),
            width: 180,
            render: (_, r) =>
              reviewable(r.result.verdict) ? (
                <ReviewCell result={r.result} onReview={onReview} saving={saving} />
              ) : (
                <VerdictTag v={effectiveVerdict(r.result)} />
              ),
          },
          {
            title: t('lint.report.reason'),
            render: (_, r) =>
              r.result.issues.length
                ? t('lint.run.issue_count', { count: r.result.issues.length })
                : r.result.review?.note || r.result.reason || '',
          },
        ]}
      />
    </div>
  )
}

function Issues({ result }: { result: LintClauseResult }) {
  const { t } = useTranslation()
  return (
    <Table<LintIssue>
      size="small"
      pagination={false}
      rowKey={(_, i) => String(i)}
      dataSource={result.issues}
      columns={[
        { title: t('lint.report.object_col'), dataIndex: 'object', width: 260, render: (v: string) => <code>{v}</code> },
        { title: t('lint.report.message'), dataIndex: 'message' },
        {
          title: t('lint.report.suggestion'),
          dataIndex: 'suggestion',
          render: (v?: string | null) => <span style={{ color: '#888' }}>{v || ''}</span>,
        },
      ]}
    />
  )
}

function ReviewCell({
  result,
  onReview,
  saving,
}: {
  result: LintClauseResult
  onReview: (clause: string, verdict: LintVerdict, note: string) => void
  saving: boolean
}) {
  const { t } = useTranslation()
  const [verdict, setVerdict] = useState<LintVerdict>(result.review?.verdict ?? 'pass')
  const [note, setNote] = useState(result.review?.note ?? '')
  const choices: LintVerdict[] = ['pass', 'fail', 'not_applicable']
  return (
    <Space direction="vertical" size={4} style={{ width: '100%' }}>
      <VerdictTag v={effectiveVerdict(result)} />
      <Space.Compact>
        <Select
          size="small"
          style={{ width: 100 }}
          value={verdict}
          onChange={setVerdict}
          options={choices.map((v) => ({ value: v, label: t(`lint.verdict.${v}`) }))}
        />
        <Button size="small" loading={saving} onClick={() => onReview(result.clause, verdict, note)}>
          {t('lint.run.save_review')}
        </Button>
      </Space.Compact>
      <Input size="small" placeholder={t('lint.run.review_note')} value={note} onChange={(e) => setNote(e.target.value)} />
    </Space>
  )
}
