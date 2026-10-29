// 文档导出任务的进度与结果：右下角的通知轮询任务状态，可取消；完成后桌面版可在文件夹中显示，浏览器版下载。

import { App, Button } from 'antd'
import { useTranslation } from 'react-i18next'

import { useApi } from '../../app/context'
import { errorText } from '../../shared/api'
import { desktop } from '../../shared/shell'

const sleep = (ms: number) => new Promise((r) => setTimeout(r, ms))

export function useDocTracker() {
  const api = useApi()
  const { t } = useTranslation()
  const { notification } = App.useApp()

  /** 跟踪任务直到结束。`output` 是桌面版写到的路径。 */
  return async (id: string, title: string, output: string | null) => {
    const key = `doc-${id}`
    await sleep(300)
    for (;;) {
      const st = await api.task(id).catch(() => null)
      if (!st) return
      if (st.state === 'running') {
        const { read, total } = st.progress
        notification.open({
          key,
          message: title,
          description: st.phase === 'scanning' ? t('document.task.reading', { read, total }) : t('document.task.writing'),
          duration: 0,
          actions: (
            <Button size="small" onClick={() => void api.cancelTask(id)}>
              {t('document.task.cancel')}
            </Button>
          ),
        })
        await sleep(800)
        continue
      }
      if (st.state === 'done') {
        const warn = st.warnings ? t('document.task.truncated', { count: st.warnings }) : null
        if (!output && st.report) {
          const a = document.createElement('a')
          a.href = api.taskReportUrl(st.id)
          a.download = st.report
          a.click()
        }
        notification.success({
          key,
          message: t('document.task.done', { title }),
          description: [output, warn].filter(Boolean).map((l) => <div key={l}>{l}</div>),
          actions:
            output && desktop ? (
              <Button size="small" onClick={() => void desktop!.revealInFolder(output)}>
                {t('document.task.reveal')}
              </Button>
            ) : undefined,
          duration: 0,
        })
      } else if (st.state === 'failed') {
        notification.error({
          key,
          message: t('document.task.failed', { title }),
          description: st.error ? errorText(st.error) : null,
          duration: 0,
        })
      } else {
        notification.info({ key, message: t('document.task.cancelled', { title }), duration: 6 })
      }
      return
    }
  }
}
