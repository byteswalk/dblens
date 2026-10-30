// 检查任务的进度与结果：右下角通知轮询任务状态，可取消；完成后打开结果标签。

import { App, Button } from 'antd'
import { useTranslation } from 'react-i18next'

import { useApi } from '../../app/context'
import { errorText } from '../../shared/api'
import { useTabs } from '../editor/tabs'

const sleep = (ms: number) => new Promise((r) => setTimeout(r, ms))

export function useLintTracker() {
  const api = useApi()
  const { t } = useTranslation()
  const { notification } = App.useApp()

  /** 跟踪检查任务直到结束。`title` 是检查对象的显示名。 */
  return async (id: string, title: string) => {
    const key = `lint-${id}`
    await sleep(300)
    for (;;) {
      const st = await api.task(id).catch(() => null)
      if (!st) return
      if (st.state === 'running') {
        const { read, total } = st.progress
        notification.open({
          key,
          message: title,
          description: t('lint.task.running', { read, total }),
          duration: 0,
          actions: (
            <Button size="small" onClick={() => void api.cancelTask(id)}>
              {t('lint.task.cancel')}
            </Button>
          ),
        })
        await sleep(600)
        continue
      }
      if (st.state === 'done' && st.lint_run) {
        notification.destroy(key)
        useTabs.getState().openLintRun(st.lint_run, t('lint.run.title', { target: title }))
      } else if (st.state === 'failed') {
        notification.error({
          key,
          message: t('lint.task.failed', { title }),
          description: st.error ? errorText(st.error) : null,
          duration: 0,
        })
      } else {
        notification.destroy(key)
      }
      return
    }
  }
}
