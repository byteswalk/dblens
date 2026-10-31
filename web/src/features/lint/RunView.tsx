// 检查结果：总分统计、按分类分组的条款、问题明细、人工结论、例外、导出报告。

import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { App, Button, Dropdown, Empty, Input, Modal, Select, Space, Spin, Table, Tag } from 'antd'
import { useState } from 'react'
import { useTranslation } from 'react-i18next'

import { useApi } from '../../app/context'
import type { LintClauseResult, LintIssue, LintVerdict, LintWaivedIssue } from '../../shared/api'
import { desktop } from '../../shared/shell'
import type { QueryTab } from '../editor/tabs'
import { download, effectiveVerdict, groupBySection, reviewable, summarize, verdictTone } from './logic'

const TONE: Record<string, string> = { ok: '#2a7d4f', bad: '#c0392b', warn: '#b8860b', muted: '#999' }
const REPORT_FORMATS = ['html', 'docx', 'xlsx', 'checklist'] as const

/** 豁免、撤销的回调，沿表格传下去。 */
interface WaiveActions {
  onWaive: (clause: string, object: string) => void
  onUnwaive: (clause: string, object: string) => void
}

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
  const [waiving, setWaiving] = useState<{ clause: string; object: string } | null>(null)
  const [waiveReason, setWaiveReason] = useState('')

  const key = ['lint', 'run', runId]
  const { data: run, isLoading, error } = useQuery({ queryKey: key, queryFn: () => api.lintRun(runId), enabled: !!runId })

  const review = useMutation({
    mutationFn: ({ clause, verdict, note }: { clause: string; verdict: LintVerdict; note: string }) =>
      api.reviewLintClause(runId, clause, { verdict, note }),
    onSuccess: (updated) => qc.setQueryData(key, updated),
    onError: (e) => message.error(e instanceof Error ? e.message : String(e)),
  })

  const afterWaiver = (updated: Awaited<ReturnType<typeof api.lintRun>>) => {
    qc.setQueryData(key, updated)
    void qc.invalidateQueries({ queryKey: ['lint', 'ruleset', updated.ruleset_id] })
    void qc.invalidateQueries({ queryKey: ['lint', 'rulesets'] })
  }
  const waive = useMutation({
    mutationFn: ({ clause, object, reason }: { clause: string; object: string; reason: string }) =>
      api.waiveLintIssue(runId, clause, object, reason),
    onSuccess: (updated) => {
      afterWaiver(updated)
      setWaiving(null)
    },
    onError: (e) => message.error(e instanceof Error ? e.message : String(e)),
  })
  const unwaive = useMutation({
    mutationFn: ({ clause, object }: { clause: string; object: string }) =>
      api.unwaiveLintIssue(runId, clause, object),
    onSuccess: afterWaiver,
    onError: (e) => message.error(e instanceof Error ? e.message : String(e)),
  })

  if (isLoading) return <Spin style={{ margin: 48 }} />
  if (error || !run)
    return <Empty style={{ marginTop: 48 }} description={error instanceof Error ? error.message : t('lint.run.missing')} />

  const s = summarize(run.results)
  const tiles: [LintVerdict | 'issues' | 'waived', number][] = [
    ['pass', s.pass],
    ['fail', s.fail],
    ['unknown', s.unknown],
    ['manual', s.manual],
    ['not_applicable', s.not_applicable],
    ['issues', s.issues],
    ['waived', s.waived],
  ]
  const actions: WaiveActions = {
    onWaive: (clause, object) => {
      setWaiveReason('')
      setWaiving({ clause, object })
    },
    onUnwaive: (clause, object) => unwaive.mutate({ clause, object }),
  }

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
            <Dropdown
              trigger={['click']}
              menu={{
                items: REPORT_FORMATS.map((f) => ({ key: f, label: t(`lint.run.format.${f}`) })),
                onClick: ({ key }) => {
                  download(api.lintReportUrl(runId, key, reviewer || undefined))
                  if (desktop) message.success(t('lint.downloaded'))
                },
              }}
            >
              <Button size="small">{t('lint.run.report')}</Button>
            </Dropdown>
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
              {k === 'issues' || k === 'waived' ? t(`lint.report.${k}`) : t(`lint.verdict.${k}`)}: <b>{n}</b>
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
            actions={actions}
          />
        ))}
      </Space>

      <Modal
        open={!!waiving}
        title={t('lint.run.waive_title', { object: waiving?.object ?? '' })}
        okText={t('lint.run.waive')}
        okButtonProps={{ disabled: !waiveReason.trim() }}
        confirmLoading={waive.isPending}
        onOk={() => waiving && waive.mutate({ ...waiving, reason: waiveReason })}
        onCancel={() => setWaiving(null)}
        destroyOnHidden
      >
        <Input.TextArea
          autoFocus
          rows={3}
          placeholder={t('lint.run.waive_reason')}
          value={waiveReason}
          onChange={(e) => setWaiveReason(e.target.value)}
        />
      </Modal>
    </div>
  )
}

function Section({
  title,
  rows,
  onReview,
  saving,
  actions,
}: {
  title: string
  rows: Row[]
  onReview: (clause: string, verdict: LintVerdict, note: string) => void
  saving: boolean
  actions: WaiveActions
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
          rowExpandable: (r) => r.result.issues.length > 0 || (r.result.waived?.length ?? 0) > 0,
          expandedRowRender: (r) => <Issues result={r.result} actions={actions} />,
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
            render: (_, r) => {
              const waived = r.result.waived?.length ?? 0
              const text = r.result.issues.length
                ? t('lint.run.issue_count', { count: r.result.issues.length })
                : r.result.review?.note || r.result.reason || ''
              return [text, waived ? t('lint.run.waived_count', { count: waived }) : ''].filter(Boolean).join(' · ')
            },
          },
        ]}
      />
    </div>
  )
}

function Issues({ result, actions }: { result: LintClauseResult; actions: WaiveActions }) {
  const { t } = useTranslation()
  // 右键时记下行，菜单作用在这一行的对象上
  const [target, setTarget] = useState<string | null>(null)
  const waived = result.waived ?? []
  return (
    <Space direction="vertical" size={8} style={{ width: '100%' }}>
      {result.issues.length > 0 && (
        <Dropdown
          trigger={['contextMenu']}
          menu={{
            items: target ? [{ key: 'waive', label: t('lint.run.waive_menu') }] : [],
            onClick: () => target && actions.onWaive(result.clause, target),
          }}
        >
          <div onContextMenuCapture={() => setTarget(null)}>
            <Table<LintIssue>
              size="small"
              pagination={false}
              rowKey={(_, i) => String(i)}
              dataSource={result.issues}
              onRow={(r) => ({ onContextMenu: () => setTarget(r.object) })}
              footer={() => <span style={{ color: '#999', fontSize: 12 }}>{t('lint.run.waive_hint')}</span>}
              columns={[
                {
                  title: t('lint.report.object_col'),
                  dataIndex: 'object',
                  width: 260,
                  render: (v: string) => <code>{v}</code>,
                },
                { title: t('lint.report.message'), dataIndex: 'message' },
                {
                  title: t('lint.report.suggestion'),
                  dataIndex: 'suggestion',
                  render: (v?: string | null) => <span style={{ color: '#888' }}>{v || ''}</span>,
                },
              ]}
            />
          </div>
        </Dropdown>
      )}
      {waived.length > 0 && (
        <Table<LintWaivedIssue>
          size="small"
          pagination={false}
          rowKey={(_, i) => String(i)}
          dataSource={waived}
          columns={[
            {
              title: t('lint.report.waived'),
              dataIndex: 'object',
              width: 260,
              render: (v: string) => <code>{v}</code>,
            },
            { title: t('lint.report.message'), dataIndex: 'message' },
            { title: t('lint.report.waive_reason'), dataIndex: 'reason' },
            {
              title: '',
              width: 80,
              render: (_: unknown, w) => (
                <Button type="link" size="small" onClick={() => actions.onUnwaive(result.clause, w.object)}>
                  {t('lint.run.unwaive')}
                </Button>
              ),
            },
          ]}
        />
      )}
    </Space>
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
