// 迁移的进度与未完成的迁移（M5b 设计文档第 6 节）：执行中按表列出状态、已迁行数 / 估计、跳过、用时、校验结果；
// 标签顶部列出没有完成的迁移，可继续或放弃。

import { useQuery, useQueryClient } from '@tanstack/react-query'
import { Alert, App, Button, Space, Table, Tag } from 'antd'
import { useTranslation } from 'react-i18next'

import { useApi } from '../../app/context'
import type { MigrationProgress } from '../../shared/api'
import { withVault } from '../connections/vault'

const STATE_COLOR: Record<MigrationProgress['tables'][number]['state'], string> = {
  pending: 'default',
  copying: 'processing',
  done: 'success',
  failed: 'error',
  skipped: 'default',
}

export function ProgressTable({ progress }: { progress: MigrationProgress }) {
  const { t } = useTranslation()
  if (!progress.tables.length) return null
  return (
    <div className="migrate-progress">
      <b>{t(`migrate.phase.${progress.phase}`)}</b>
      <Table
        size="small"
        pagination={false}
        rowKey="source"
        dataSource={progress.tables}
        columns={[
          { title: t('migrate.progress.table'), render: (_, r) => `${r.source} → ${r.target}` },
          {
            title: t('migrate.progress.state'),
            render: (_, r) => (
              <Tag color={STATE_COLOR[r.state]} title={r.error ?? undefined}>
                {t(`migrate.state.${r.state}`)}
              </Tag>
            ),
          },
          { title: t('migrate.progress.rows'), render: (_, r) => `${r.rows.toLocaleString()}${r.estimate !== null ? ` / ${r.estimate.toLocaleString()}` : ''}` },
          { title: t('migrate.progress.skipped'), render: (_, r) => r.skipped || '' },
          { title: t('migrate.progress.time'), render: (_, r) => `${(r.elapsed_ms / 1000).toFixed(1)} s` },
          {
            title: t('migrate.progress.verify'),
            render: (_, r) =>
              r.error && r.state !== 'done' ? (
                <span className="err">{r.error}</span>
              ) : !r.verify ? null : r.verify.target_rows === r.verify.expected_rows && !r.verify.mismatches.length ? (
                <Tag color="success">{t('migrate.progress.verifyOk')}</Tag>
              ) : (
                <span className="err">
                  {t('migrate.progress.verifyBad', {
                    expected: r.verify.expected_rows,
                    target: r.verify.target_rows,
                    mismatches: r.verify.mismatches.length,
                  })}
                </span>
              ),
          },
        ]}
      />
    </div>
  )
}

/** 没有完成的迁移：继续时把任务 id 交给 `onResume`。 */
export function UnfinishedList({ onResume }: { onResume(task: string): void }) {
  const api = useApi()
  const { t } = useTranslation()
  const { message, modal } = App.useApp()
  const qc = useQueryClient()
  const { data = [] } = useQuery({ queryKey: ['migrations'], queryFn: () => api.unfinishedMigrations() })
  if (!data.length) return null
  const refresh = () => void qc.invalidateQueries({ queryKey: ['migrations'] })
  return (
    <Alert
      type="info"
      showIcon
      message={t('migrate.unfinished.title')}
      description={data.map((m) => (
        <div key={m.id} className="row" style={{ gap: 8 }}>
          <span className="grow">
            {t('migrate.unfinished.item', {
              source: m.source,
              target: m.target,
              phase: t(`migrate.phase.${m.phase}`),
              done: m.tables_done,
              total: m.tables_total,
            })}
          </span>
          {m.running ? (
            <Tag color="processing">{t('migrate.unfinished.running')}</Tag>
          ) : (
            <Space>
              <Button
                size="small"
                type="primary"
                onClick={async () => {
                  try {
                    onResume(await withVault(api, () => api.resumeMigration(m.id)))
                    refresh()
                  } catch (e) {
                    message.error((e as Error).message)
                  }
                }}
              >
                {t('migrate.unfinished.resume')}
              </Button>
              <Button
                size="small"
                danger
                onClick={() =>
                  modal.confirm({
                    title: t('migrate.unfinished.discard'),
                    content: t('migrate.unfinished.discardConfirm'),
                    okButtonProps: { danger: true },
                    onOk: async () => {
                      await api.discardMigration(m.id).catch((e) => message.error((e as Error).message))
                      refresh()
                    },
                  })
                }
              >
                {t('migrate.unfinished.discard')}
              </Button>
            </Space>
          )}
        </div>
      ))}
    />
  )
}
