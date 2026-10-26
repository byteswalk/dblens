// 主线程一侧的 SQL 语言服务：把请求发给 worker，按序号对应结果（M1b2 设计文档第 4、7 节）。
// - 所有方言共用一个 worker，解析器在 worker 中按需加载；
// - 某个方言第一次使用时先加载解析器（不计超时），之后每次请求超过 2 秒就放弃结果；
// - worker 出错或某个方言加载失败后，该方言在本次运行中不再使用解析器（调用方退回轻量补全、不检查语法）。

import type { Request, RequestBody, Response } from './protocol'
import type { Dialect, ParseIssue, ParserLocale, StatementRange, Suggest } from './types'

export const TIMEOUT_MS = 2000
/** 加载解析器（下载 + 预热）最多等这么久。 */
const LOAD_TIMEOUT_MS = 60_000

/** worker 的最小接口，测试中用假的实现。 */
export interface WorkerLike {
  postMessage(msg: Request): void
  onmessage: ((e: MessageEvent<Response>) => void) | null
  onerror: ((e: ErrorEvent) => void) | null
}

/** 加载失败的标记。 */
const FAILED = Promise.resolve(false)

export type Outcome<T> = { ok: true; value: T } | { ok: false; reason: 'timeout' | 'unavailable' }

export class SqlClient {
  private worker: WorkerLike | null = null
  private seq = 0
  private pending = new Map<number, (r: Response | null) => void>()
  private loads = new Map<Dialect, Promise<boolean>>()
  private broken = false

  constructor(private readonly create: () => WorkerLike) {}

  /** 该方言的解析器是否不可用（加载失败或 worker 出错）。 */
  unavailable(d: Dialect): boolean {
    return this.broken || this.loads.get(d) === FAILED
  }

  validate(dialect: Dialect, sql: string, locale: ParserLocale): Promise<Outcome<ParseIssue[]>> {
    return this.call(dialect, { op: 'validate', dialect, sql, locale })
  }

  suggest(dialect: Dialect, sql: string, offset: number): Promise<Outcome<Suggest>> {
    return this.call(dialect, { op: 'suggest', dialect, sql, offset })
  }

  statementAt(dialect: Dialect, sql: string, offset: number): Promise<Outcome<StatementRange | null>> {
    return this.call(dialect, { op: 'statement', dialect, sql, offset })
  }

  private async call<T>(dialect: Dialect, req: RequestBody): Promise<Outcome<T>> {
    if (!(await this.load(dialect))) return { ok: false, reason: 'unavailable' }
    const resp = await this.send(req, TIMEOUT_MS)
    if (resp === null) return { ok: false, reason: this.broken ? 'unavailable' : 'timeout' }
    if (!resp.ok) return { ok: false, reason: 'unavailable' }
    return { ok: true, value: resp.result as T }
  }

  private load(d: Dialect): Promise<boolean> {
    let p = this.loads.get(d)
    if (!p) {
      p = this.send({ op: 'load', dialect: d }, LOAD_TIMEOUT_MS).then((r) => {
        if (r?.ok) return true
        console.warn(`SQL 解析器（${d}）不可用`, r && !r.ok ? r.error : '超时')
        this.loads.set(d, FAILED)
        return false
      })
      this.loads.set(d, p)
    }
    return p
  }

  /** 发送请求；超时或 worker 出错时得到 null。 */
  private send(req: RequestBody, timeoutMs: number): Promise<Response | null> {
    if (this.broken) return Promise.resolve(null)
    const w = this.ensureWorker()
    const id = ++this.seq
    return new Promise((resolve) => {
      const timer = setTimeout(() => done(null), timeoutMs)
      const done = (r: Response | null) => {
        clearTimeout(timer)
        this.pending.delete(id)
        resolve(r)
      }
      this.pending.set(id, done)
      w.postMessage({ ...req, id } as Request)
    })
  }

  private ensureWorker(): WorkerLike {
    if (!this.worker) {
      const w = this.create()
      w.onmessage = (e) => this.pending.get(e.data.id)?.(e.data)
      w.onerror = (e) => {
        console.warn('SQL 语言 worker 出错', e)
        this.broken = true
        for (const done of [...this.pending.values()]) done(null)
      }
      this.worker = w
    }
    return this.worker
  }
}
