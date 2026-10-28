// 同步到目标（M4a 设计文档第 6 节）：目标是库时预览脚本（不能重建的表、执行策略的结论、提示），确认后作为任务执行并显示进度；
// 目标是模型时列出要写的表文件，确认后直接写回。引入 Monaco（脚本预览），由比对标签懒加载。

import { useQuery } from '@tanstack/react-query'
import { Alert, App, Button, Modal, Progress, Spin } from 'antd'
import { useEffect, useState } from 'react'
import { useTranslation } from 'react-i18next'

import { useApi } from '../../app/context'
import { type DriverId, type SyncRequest, type TaskStatus, errorText } from '../../shared/api'
import SqlBox from '../design/SqlBox'
import { useQueryActions } from '../editor/actions'

export default function SyncDialog({ request, driver, profileId, onDone, onClose }: {
  request: SyncRequest
  /** 目标是库时的驱动（脚本高亮）。 */
  driver: DriverId | undefined
  profileId: string | null
  onDone(): void
  onClose(): void
}) {
  const api = useApi()
  const { t } = useTranslation()
  const { message } = App.useApp()
  const { openSql } = useQueryActions()
  const [task, setTask] = useState<TaskStatus | null>(null)
  const [busy, setBusy] = useState(false)
  const { data: p, error, isFetching } = useQuery({
    queryKey: ['sync-preview', JSON.stringify(request)],
    queryFn: () => api.syncPreview(request),
    retry: false,
  })

  useEffect(() => {
    if (!task) return
    if (task.state === 'done') {
      message.success(t('model.sync.done'))
      onDone()
      return
    }
    if (task.state !== 'running') return
    const h = setTimeout(async () => {
      try {
        setTask(await api.task(task.id))
      } catch (e) {
        message.error((e as Error).message)
        setTask(null)
      }
    }, 400)
    return () => clearTimeout(h)
  }, [task, api, message, t, onDone])

  const run = async () => {
    setBusy(true)
    try {
      const id = await api.sync({ ...request, fingerprint: p?.fingerprint ?? null })
      if (id) setTask({ id, state: 'running', progress: { statements_done: 0, statements_total: 0 } } as TaskStatus)
      else {
        message.success(t('model.sync.written'))
        onDone()
      }
    } catch (e) {
      message.error((e as Error).message)
    } finally {
      setBusy(false)
    }
  }

  const script = p?.script
  const blocked = !p || !!p.denied || !!script?.refused.length || (script ? !script.steps.length : !p.writes.length)
  const running = task?.state === 'running'
  return (
    <Modal
      open
      width={820}
      title={t('model.sync.title')}
      onCancel={onClose}
      okText={script ? t('model.sync.execute') : t('model.sync.write')}
      okButtonProps={{ disabled: blocked || running || !!task, loading: busy || running, danger: !!p?.confirm.length }}
      onOk={() => void run()}
    >
      {error ? (
        <Alert type="error" message={(error as Error).message} />
      ) : !p || isFetching ? (
        <Spin />
      ) : (
        <div className="design-save">
          {p.denied && <Alert type="error" message={t('model.sync.denied', { reason: p.denied })} />}
          {script?.refused.map(([table, reason]) => (
            <Alert key={table} type="error" message={t('model.sync.refused', { table, reason })} />
          ))}
          {p.confirm.map((c) => (
            <Alert key={c} type="warning" showIcon message={c} />
          ))}
          {script?.warnings.map((w) => (
            <Alert key={w} type="warning" showIcon message={w} />
          ))}
          {script ? (
            <>
              <Alert type="info" showIcon message={t(script.transactional ? 'design.sql.transactional' : 'design.sql.autocommit')} />
              <SqlBox sql={p.text} driver={driver} height={360} />
            </>
          ) : (
            <ul className="design-changes">
              {p.writes.map((w) => (
                <li key={w.name} className={w.delete ? 'err' : undefined}>
                  {w.delete
                    ? t('model.sync.deleteFile', { name: w.name })
                    : w.from
                      ? t('model.sync.renameFile', { from: w.from, name: w.name })
                      : t('model.sync.writeFile', { name: w.name })}
                </li>
              ))}
            </ul>
          )}
          {running && (
            <Progress
              size="small"
              percent={Math.round((100 * task.progress.statements_done) / Math.max(1, task.progress.statements_total))}
            />
          )}
          {task && !running && task.state !== 'done' && (
            <Alert
              type={task.state === 'cancelled' ? 'warning' : 'error'}
              message={task.state === 'cancelled' ? t('design.run.cancelled') : t('design.run.failed')}
              description={
                <>
                  {task.failures[0] && (
                    <div>
                      {t('design.run.failedAt', { n: task.failures[0].index + 1 })}
                      <code className="design-failed-sql">{task.failures[0].text}</code>
                    </div>
                  )}
                  {task.error && <div className="err">{errorText(task.error)}</div>}
                </>
              }
              action={
                task.remaining && profileId ? (
                  <Button size="small" onClick={() => openSql(task.remaining ?? '', profileId)}>
                    {t('design.run.remaining')}
                  </Button>
                ) : undefined
              }
            />
          )}
          {running && (
            <Button size="small" onClick={() => void api.cancelTask(task.id).catch((e) => message.error((e as Error).message))}>
              {t('design.run.cancel')}
            </Button>
          )}
        </div>
      )}
    </Modal>
  )
}
