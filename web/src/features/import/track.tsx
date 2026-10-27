// 后台任务的进度与结果（M2b 设计文档 3.3、3.4 节）：右下角的通知轮询任务状态，显示进度、可取消；
// SQL 文件需要确认时弹确认框；结束后显示结果，可以下载出错报告或执行日志、查看失败的语句、打开导入的表。

import { App, Button, Modal } from 'antd'
import { useTranslation } from 'react-i18next'

import { useApi } from '../../app/context'
import { useUi } from '../../app/ui'
import { type TaskStatus, errorText } from '../../shared/api'
import { desktop } from '../../shared/shell'
import { useQueryActions } from '../editor/actions'
import { useTabs } from '../editor/tabs'

const sleep = (ms: number) => new Promise((r) => setTimeout(r, ms))

/** 任务的上下文：通知的标题，结束后刷新对象树、打开数据用。 */
export interface TaskContext {
  title: string
  profileId: string
  schema: string | null
  /** 导入的目标表。 */
  table?: string
  /** 导入时新建了表，或执行了 SQL 文件：结束后刷新对象树。 */
  refreshTree?: boolean
}

/** 已读的比例（0–100）；不知道时为 null。 */
export function percent(st: TaskStatus): number | null {
  const { read, total } = st.progress
  return total > 0 ? Math.min(100, Math.floor((read / total) * 100)) : null
}

export function useTaskTracker() {
  const api = useApi()
  const { t } = useTranslation()
  const { notification, modal, message } = App.useApp()
  const { openSql } = useQueryActions()

  const progressText = (st: TaskStatus): string => {
    const p = st.progress
    const pct = percent(st)
    const tail = pct === null ? '' : ` · ${pct}%`
    if (st.phase === 'scanning') return t('import.task.scanning') + tail
    if (st.phase === 'creating') return t('import.task.creating')
    if (st.phase === 'clearing') return t('import.task.clearing')
    if (st.kind === 'sql_file') {
      return t('import.task.statements', { done: p.statements_done, total: p.statements_total, failed: p.statements_failed }) + tail
    }
    return t('import.task.rows', { read: p.rows_read, written: p.rows_written, skipped: p.rows_skipped }) + tail
  }

  const downloadReport = async (st: TaskStatus) => {
    const name = st.report
    if (!name) return
    if (desktop) {
      const ext = name.split('.').pop() ?? 'txt'
      const path = await desktop.saveDialog(name, [{ name, extensions: [ext] }])
      if (!path) return
      try {
        await api.saveTaskReport(st.id, path)
        void desktop.revealInFolder(path)
      } catch (e) {
        message.error((e as Error).message)
      }
      return
    }
    const a = document.createElement('a')
    a.href = api.taskReportUrl(st.id)
    a.download = name
    a.click()
  }

  const showFailures = (st: TaskStatus, ctx: TaskContext) => {
    modal.info({
      title: t('import.sql.failuresTitle', { count: st.progress.statements_failed }),
      width: 760,
      content: (
        <div style={{ maxHeight: 420, overflow: 'auto' }}>
          {st.failures.map((f, n) => (
            <div key={n} style={{ borderBottom: '1px solid var(--border)', padding: '8px 0' }}>
              <div className="row" style={{ gap: 8 }}>
                <b>{t('import.sql.line', { line: f.line })}</b>
                <span className="grow" />
                <Button
                  size="small"
                  onClick={async () => {
                    const sql = await api.taskStatement(st.id, n).catch(() => f.text)
                    Modal.destroyAll()
                    openSql(sql, ctx.profileId)
                  }}
                >
                  {t('import.sql.openInQuery')}
                </Button>
              </div>
              <div className="err">{errorText(f.error)}</div>
              <pre className="mono" style={{ whiteSpace: 'pre-wrap', margin: 0 }}>{f.text}</pre>
            </div>
          ))}
          {st.progress.statements_failed > st.failures.length && (
            <div className="muted" style={{ marginTop: 8 }}>{t('import.sql.moreInLog')}</div>
          )}
        </div>
      ),
    })
  }

  const ask = (st: TaskStatus) => {
    const c = st.confirm
    if (!c) return
    modal.confirm({
      title: t('import.sql.confirmTitle'),
      width: 640,
      content: (
        <div>
          <div>{t('import.sql.confirmContent', { total: st.progress.statements_total })}</div>
          {c.items.length > 0 && (
            <ul style={{ paddingLeft: 18, maxHeight: 280, overflow: 'auto' }}>
              {c.items.map((i) => (
                <li key={i.line}>
                  <span className="muted">{t('import.sql.line', { line: i.line })}</span>{' '}
                  <code>{i.text}</code> <span className="err">{i.reason}</span>
                </li>
              ))}
            </ul>
          )}
          {c.more > 0 && <div className="muted">{t('import.sql.confirmMore', { count: c.more })}</div>}
        </div>
      ),
      okText: t('import.sql.confirmOk'),
      okType: 'danger',
      onOk: () => api.confirmTask(st.id, true),
      onCancel: () => void api.confirmTask(st.id, false).catch(() => {}),
    })
  }

  const finish = (st: TaskStatus, ctx: TaskContext, key: string) => {
    const p = st.progress
    const lines: string[] = []
    if (st.kind === 'import') {
      lines.push(t('import.result.written', { count: p.rows_written }))
      if (p.rows_skipped) lines.push(t('import.result.skipped', { count: p.rows_skipped }))
    } else {
      lines.push(t('import.sql.result', { ok: p.statements_done - p.statements_failed, failed: p.statements_failed, affected: p.affected }))
    }
    if (st.warnings) lines.push(t('import.result.warnings', { count: st.warnings }))
    if (st.state === 'failed') {
      if (st.error) lines.push(errorText(st.error))
      if (st.error_line !== null) lines.push(t('import.result.errorLine', { line: st.error_line }))
      if (st.committed_line !== null) lines.push(t('import.result.committed', { line: st.committed_line }))
    }
    if (ctx.refreshTree) useUi.getState().refreshTree(ctx.profileId, ctx.schema)
    const actions = (
      <span className="row" style={{ gap: 8 }}>
        {st.report && (
          <Button size="small" onClick={() => void downloadReport(st)}>
            {st.kind === 'import' ? t('import.result.report') : t('import.sql.log')}
          </Button>
        )}
        {st.kind === 'sql_file' && st.failures.length > 0 && (
          <Button size="small" onClick={() => showFailures(st, ctx)}>
            {t('import.sql.failures')}
          </Button>
        )}
        {st.kind === 'import' && st.state === 'done' && ctx.table && ctx.schema && (
          <Button
            size="small"
            onClick={() =>
              useTabs.getState().openData(ctx.profileId, { schema: ctx.schema!, object: ctx.table!, objectKind: 'table' })
            }
          >
            {t('import.result.openData')}
          </Button>
        )}
      </span>
    )
    const description = lines.map((l) => <div key={l}>{l}</div>)
    if (st.state === 'done') {
      notification.success({ key, message: t('import.task.done', { title: ctx.title }), description, actions, duration: 0 })
    } else if (st.state === 'failed') {
      notification.error({ key, message: t('import.task.failed', { title: ctx.title }), description, actions, duration: 0 })
    } else {
      notification.info({ key, message: t('import.task.cancelled', { title: ctx.title }), description, actions, duration: 8 })
    }
  }

  /** 跟踪一个任务直到结束。 */
  return async (id: string, ctx: TaskContext) => {
    const key = `task-${id}`
    let asked = false
    await sleep(300)
    for (;;) {
      const st = await api.task(id).catch(() => null)
      if (!st) return
      if (st.state === 'running' || st.state === 'confirm') {
        if (st.state === 'confirm' && !asked) {
          asked = true
          ask(st)
        }
        notification.open({
          key,
          message: ctx.title,
          description: st.state === 'confirm' ? t('import.task.waiting') : progressText(st),
          duration: 0,
          actions: (
            <Button size="small" onClick={() => void api.cancelTask(id)}>
              {t('import.task.cancel')}
            </Button>
          ),
        })
        await sleep(1000)
        continue
      }
      finish(st, ctx, key)
      return
    }
  }
}
