// 前端错误写进后端日志：界面崩溃、未捕获的异常和 Promise 拒绝。
// 日志文件在数据目录的 logs 下（浏览器模式打印到 dblens-server 的控制台）。

export interface ClientError {
  kind: 'render' | 'error' | 'rejection'
  message: string
  stack?: string
  component_stack?: string
}

type Send = (e: ClientError) => Promise<unknown>

/** 浏览器的提示，不是错误。 */
const IGNORED = /ResizeObserver loop/
/** 一次运行最多上报的条数，防止循环报错刷爆日志。 */
const MAX_REPORTS = 30

let send: Send | null = null
let reported = 0
const seen = new Set<string>()

export function reportClientError(e: ClientError) {
  console.error(`[${e.kind}]`, e.message, e.stack ?? '')
  const key = `${e.kind}:${e.message}`
  if (!send || IGNORED.test(e.message) || seen.has(key) || reported >= MAX_REPORTS) return
  seen.add(key)
  reported += 1
  // 上报失败时不能再触发上报
  send(e).catch((failed: unknown) => console.warn('上报前端错误失败', failed))
}

/** 只装一次；重复调用时换用新的发送函数。 */
export function installCrashLog(next: Send) {
  const first = send === null
  send = next
  if (!first) return
  window.addEventListener('error', (e) => {
    reportClientError({ kind: 'error', message: e.message, stack: (e.error as Error | undefined)?.stack })
  })
  window.addEventListener('unhandledrejection', (e) => {
    const r = e.reason as Error | string | undefined
    reportClientError({
      kind: 'rejection',
      message: typeof r === 'string' ? r : (r?.message ?? String(r)),
      stack: typeof r === 'string' ? undefined : r?.stack,
    })
  })
}
