// 表设计器的规范提示（M7 设计文档第 2 节 M7c2）：改动后约 300 毫秒自动检查这张表，只提示，不阻止保存。
// 只跑单表规则，一致性和其他对象的规则要做整库检查。

import { keepPreviousData, useQuery } from '@tanstack/react-query'
import { Alert, Empty, Select, Table } from 'antd'
import { useTranslation } from 'react-i18next'

import { useApi } from '../../app/context'
import type { DesignTableModel, LintSource, LintTableHit } from '../../shared/api'
import { useSettings } from '../../shared/settings'

const RULESET_KEY = 'lint.design.ruleset'
const DEFAULT_RULESET = 'general'

interface Row {
  key: string
  hit: LintTableHit
  object: string
  message: string
  suggestion?: string | null
}

/** 检查草稿。`source` 为 null（还没选 schema）或表没有名字时不查。 */
export function useDesignLint(source: LintSource | null, draft: DesignTableModel | undefined) {
  const api = useApi()
  const saved = useSettings((s) => s.values[RULESET_KEY])
  const rulesets = useQuery({ queryKey: ['lint', 'rulesets'], queryFn: () => api.lintRulesets() })
  const ruleset = rulesets.data?.some((r) => r.id === saved) ? (saved as string) : DEFAULT_RULESET
  const hits = useQuery({
    queryKey: ['design-lint', ruleset, source, JSON.stringify(draft)],
    queryFn: () => api.lintTable({ ruleset, source: source as LintSource, table: draft as DesignTableModel }),
    enabled: !!source && !!draft?.name,
    placeholderData: keepPreviousData,
    retry: false,
  })
  const count = (hits.data ?? []).reduce((n, h) => n + h.issues.length, 0)
  return { ruleset, rulesets: rulesets.data ?? [], hits, count }
}

export default function DesignLintPane({ lint }: { lint: ReturnType<typeof useDesignLint> }) {
  const { t } = useTranslation()
  const rows: Row[] = (lint.hits.data ?? []).flatMap((hit) =>
    hit.issues.map((i, n) => ({ key: `${hit.clause}/${n}`, hit, object: i.object, message: i.message, suggestion: i.suggestion })),
  )
  return (
    <div style={{ padding: 8, overflow: 'auto', height: '100%' }}>
      <div style={{ display: 'flex', gap: 8, alignItems: 'center', marginBottom: 8 }}>
        <Select
          size="small"
          style={{ width: 200 }}
          value={lint.ruleset}
          onChange={(v) => useSettings.getState().set(RULESET_KEY, v)}
          options={lint.rulesets.map((r) => ({ value: r.id, label: r.name }))}
        />
        <span style={{ color: '#888', fontSize: 12 }}>{t('design.lint.hint')}</span>
      </div>
      {lint.hits.error ? (
        <Alert type="error" message={lint.hits.error.message} />
      ) : rows.length ? (
        <Table<Row>
          size="small"
          pagination={false}
          dataSource={rows}
          tableLayout="fixed"
          columns={[
            { title: t('lint.lib.severity'), width: 60, render: (_, r) => t(`lint.severity.${r.hit.severity}`) },
            { title: t('lint.report.clause'), render: (_, r) => r.hit.text },
            { title: t('lint.report.object_col'), width: 150, render: (_, r) => <code>{r.object}</code> },
            { title: t('lint.report.message'), dataIndex: 'message' },
            {
              title: t('lint.report.suggestion'),
              width: 120,
              dataIndex: 'suggestion',
              render: (v?: string | null) => <span style={{ color: '#888' }}>{v || ''}</span>,
            },
          ]}
        />
      ) : (
        <Empty description={t('design.lint.clean')} />
      )}
    </div>
  )
}
