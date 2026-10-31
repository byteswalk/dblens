// 检查记录：列出历史检查，打开、删除，选两次做对比。

import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { App, Button, Empty, Space, Spin, Table } from 'antd'
import { useState } from 'react'
import { useTranslation } from 'react-i18next'

import { useApi } from '../../app/context'
import type { LintRunSummary } from '../../shared/api'
import { useTabs } from '../editor/tabs'
import { formatTime, inOrder } from './logic'

export default function RunsView() {
  const api = useApi()
  const { t } = useTranslation()
  const { message, modal } = App.useApp()
  const qc = useQueryClient()
  const [selected, setSelected] = useState<string[]>([])
  const { data, isLoading } = useQuery({ queryKey: ['lint', 'runs'], queryFn: () => api.lintRuns() })

  const remove = useMutation({
    mutationFn: (id: string) => api.deleteLintRun(id),
    onSuccess: (_, id) => {
      setSelected((s) => s.filter((x) => x !== id))
      void qc.invalidateQueries({ queryKey: ['lint', 'runs'] })
    },
    onError: (e) => message.error(e instanceof Error ? e.message : String(e)),
  })

  if (isLoading) return <Spin style={{ margin: 48 }} />
  const runs = data ?? []
  if (!runs.length) return <Empty style={{ marginTop: 48 }} description={t('lint.runs.empty')} />

  const open = (r: LintRunSummary) => useTabs.getState().openLintRun(r.id, t('lint.run.title', { target: r.target }))
  const compare = () => {
    const [before, after] = inOrder(runs, selected)
    if (before && after) useTabs.getState().openLintDiff(before.id, after.id, t('lint.diff.title'))
  }

  return (
    <div style={{ padding: '12px 16px', overflow: 'auto', flex: 1, minHeight: 0 }}>
      <Space style={{ marginBottom: 8 }}>
        <Button size="small" disabled={selected.length !== 2} onClick={compare}>
          {t('lint.runs.compare')}
        </Button>
        <span style={{ color: '#888', fontSize: 12 }}>{t('lint.runs.compare_hint')}</span>
      </Space>
      <Table<LintRunSummary>
        size="small"
        pagination={false}
        rowKey="id"
        dataSource={runs}
        rowSelection={{
          selectedRowKeys: selected,
          onChange: (keys) => setSelected(keys as string[]),
          hideSelectAll: true,
          getCheckboxProps: (r) => ({ disabled: selected.length >= 2 && !selected.includes(r.id) }),
        }}
        columns={[
          { title: t('lint.runs.time'), width: 170, render: (_, r) => formatTime(r.created_at) },
          {
            title: t('lint.runs.target'),
            render: (_, r) => (
              <Button type="link" size="small" style={{ padding: 0 }} onClick={() => open(r)}>
                {r.target}
              </Button>
            ),
          },
          { title: t('lint.runs.ruleset'), dataIndex: 'ruleset_name' },
          {
            title: '',
            width: 70,
            render: (_, r) => (
              <Button
                type="link"
                size="small"
                onClick={() =>
                  modal.confirm({ title: t('lint.runs.delete_confirm'), onOk: () => remove.mutateAsync(r.id) })
                }
              >
                {t('lint.lib.delete')}
              </Button>
            ),
          },
        ]}
      />
    </div>
  )
}
