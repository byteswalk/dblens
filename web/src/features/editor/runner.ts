// 标签页的执行器：每个标签一个，持有它的会话和 WebSocket，把服务端事件归并进 useRuntime。
// 会话在第一次执行时打开（密码库锁定时先解锁）；关闭标签时关闭会话。
//
// 偏移：编辑器给的是全文中的 UTF-16 偏移。执行选中部分或确认后续跑时提交的只是全文的一段，
// 服务端事件里的偏移相对于提交的那段，收到后加上这段的起点（base），状态里存的都是全文中的偏移。

import { create } from 'zustand'

import { type Api, ApiError, type SessionInfo } from '../../shared/api'
import {
  DEFAULT_MAX_ROWS,
  type ExecOptions,
  ExecSocket,
  type ExecState,
  type ServerEvent,
  initialExec,
  reduceExec,
} from '../../shared/exec'
import { t } from '../../shared/i18n'
import { useSettings } from '../../shared/settings'
import { withVault } from '../connections/vault'
import { useTabs } from './tabs'

/** 结果行数上限的设置项，默认 10 万行（M1 设计文档第 4 节）。 */
export const ROW_CAP_KEY = 'ui.exec.row-cap'
export const DEFAULT_ROW_CAP = 100_000

export function rowCap(): number {
  const v = Number(useSettings.getState().values[ROW_CAP_KEY])
  return Number.isInteger(v) && v >= DEFAULT_MAX_ROWS ? v : DEFAULT_ROW_CAP
}

/** 一次提交：提交的 SQL、它在编辑器全文中的起点、执行选项。 */
export interface Submission {
  sql: string
  base: number
  opts: ExecOptions
}

export interface TabRuntime {
  session: SessionInfo | null
  /** 打开会话、连接服务失败等不属于某条语句的错误。 */
  error: string | null
  exec: ExecState
  running: boolean
  /** 当前结果来自执行计划。 */
  explain: boolean
  submission: Submission | null
  /** 执行时编辑器的全文：错误标记、失败语句的显示都按它取文本（之后的编辑不影响）。 */
  source: string
  continueOnError: boolean
  /** 结果行数达到上限、已停止读取的语句（statements 的下标）。 */
  capped: number | null
  /** 用户在确认框中选了取消：需要确认的那条和后面的语句都没有执行。 */
  declined: boolean
}

export const emptyRuntime: TabRuntime = {
  session: null,
  error: null,
  exec: initialExec,
  running: false,
  explain: false,
  submission: null,
  source: '',
  continueOnError: false,
  capped: null,
  declined: false,
}

interface RuntimeState {
  tabs: Record<string, TabRuntime>
  patch(id: string, p: Partial<TabRuntime>): void
  drop(id: string): void
}

export const useRuntime = create<RuntimeState>()((set, get) => ({
  tabs: {},
  patch: (id, p) => set({ tabs: { ...get().tabs, [id]: { ...(get().tabs[id] ?? emptyRuntime), ...p } } }),
  drop: (id) => {
    const tabs = { ...get().tabs }
    delete tabs[id]
    set({ tabs })
  },
}))

export function useTabRuntime(id: string | null): TabRuntime {
  return useRuntime((s) => (id ? (s.tabs[id] ?? emptyRuntime) : emptyRuntime))
}

/** 把服务端事件中的偏移换成编辑器全文中的偏移。 */
export function shiftEvent(ev: ServerEvent, base: number): ServerEvent {
  if (base === 0) return ev
  switch (ev.type) {
    case 'statement_start':
      return { ...ev, start: ev.start + base, end: ev.end + base }
    case 'confirm_required':
      return { ...ev, start: ev.start + base, end: ev.end + base }
    case 'failed':
      return ev.offset === undefined ? ev : { ...ev, offset: ev.offset + base }
    default:
      return ev
  }
}

/** 确认后接着执行的提交：执行单条语句时原样重发，否则从需要确认的那条开始截取。 */
export function resumeAfterConfirm(sub: Submission, confirmStart: number): Submission {
  const opts = { ...sub.opts, confirmed: true }
  if (sub.opts.cursor !== undefined) return { ...sub, opts }
  return { sql: sub.sql.slice(confirmStart - sub.base), base: confirmStart, opts }
}

function message(e: unknown): string {
  return e instanceof Error ? e.message : String(e)
}

export class Runner {
  private socket: ExecSocket | null = null

  constructor(
    private api: Api,
    readonly tabId: string,
  ) {}

  private rt(): TabRuntime {
    return useRuntime.getState().tabs[this.tabId] ?? emptyRuntime
  }

  private patch(p: Partial<TabRuntime>) {
    useRuntime.getState().patch(this.tabId, p)
  }

  /** 会话和 WebSocket 都就绪后返回会话；服务重启过、会话已不存在时重新打开。 */
  private async ensureSession(): Promise<SessionInfo> {
    let session = this.rt().session
    if (session && this.socket) return session
    if (session) {
      session = await this.api.session(session.id).catch((e: unknown) => {
        if (e instanceof ApiError && e.body.kind === 'not_found') return null
        throw e
      })
    }
    if (!session) {
      const tab = useTabs.getState().tabs.find((x) => x.id === this.tabId)
      if (!tab) throw new Error(t('editor.runner.tabClosed'))
      session = await withVault(this.api, () => this.api.openSession(tab.profileId))
    }
    this.patch({ session, error: null })
    this.socket = new ExecSocket(
      this.api.wsUrl(session.id),
      (ev) => this.onEvent(ev),
      () => this.onClose(),
    )
    return session
  }

  /** 执行 sql；base 是它在编辑器全文 source 中的起点。正在执行时忽略。 */
  async run(sql: string, base: number, opts: ExecOptions = {}, source = sql) {
    if (this.rt().running) return
    const submission: Submission = {
      sql,
      base,
      opts: { ...opts, continueOnError: this.rt().continueOnError, maxRows: DEFAULT_MAX_ROWS },
    }
    this.patch({
      exec: initialExec,
      running: true,
      explain: !!opts.explain,
      submission,
      source,
      capped: null,
      declined: false,
      error: null,
    })
    try {
      await this.ensureSession()
      await this.socket!.exec(submission.sql, submission.opts)
    } catch (e) {
      this.patch({ running: false, error: message(e) })
    }
  }

  /** 用户确认后，从需要确认的那条语句接着执行。 */
  async confirm() {
    const { exec, submission } = this.rt()
    if (!exec.confirm || !submission || !this.socket) return
    const next = resumeAfterConfirm(submission, exec.confirm.start)
    this.patch({ exec: { ...exec, confirm: null, done: false }, running: true, submission: next })
    await this.socket.exec(next.sql, next.opts).catch((e: unknown) => this.patch({ running: false, error: message(e) }))
  }

  /** 用户取消确认：脚本已经停下，关掉确认框，消息中说明没有执行。 */
  dismissConfirm() {
    this.patch({ declined: true })
  }

  private onEvent(raw: ServerEvent) {
    const { submission, exec, capped } = this.rt()
    const ev = shiftEvent(raw, submission?.base ?? 0)
    const next = reduceExec(exec, ev, submission?.opts.maxRows ?? DEFAULT_MAX_ROWS)
    const p: Partial<TabRuntime> = { exec: next }
    if (ev.type === 'done' || ev.type === 'error') {
      p.running = false
      void this.refreshSession()
    }
    if (next.waitingForMore && capped === null) {
      const last = next.statements.at(-1)
      if ((last?.results.at(-1)?.rows.length ?? 0) >= rowCap()) {
        // 达到行数上限：不再续取，取消这条语句（服务端在等 fetch_more，不取消的话会话一直忙）。
        // 后面还有语句的话也不再执行
        p.capped = next.statements.length - 1
        void this.socket?.cancel()
      } else if (last && !last.last) {
        // 不是最后一条：取完它的结果，后面的语句才能执行；最后一条等表格滚动到底部再续取
        p.exec = { ...next, waitingForMore: false }
        void this.socket?.fetchMore()
      }
    }
    this.patch(p)
  }

  private onClose() {
    this.socket = null
    if (this.rt().running) this.patch({ running: false, error: t('editor.runner.serviceDisconnected') })
  }

  /** 表格滚动到底部时取下一批。 */
  fetchMore() {
    const { exec, capped } = this.rt()
    if (!exec.waitingForMore || capped !== null || !this.socket) return
    this.patch({ exec: { ...exec, waitingForMore: false } })
    void this.socket.fetchMore()
  }

  cancel() {
    void this.socket?.cancel()
  }

  setContinueOnError(on: boolean) {
    this.patch({ continueOnError: on })
  }

  async refreshSession() {
    const s = this.rt().session
    if (!s) return
    const session = await this.api.session(s.id).catch(() => null)
    if (session) this.patch({ session })
  }

  /** 会话级操作：需要时先打开会话，完成后更新会话信息。出错时抛给调用方显示。 */
  private async sessionOp(f: (id: number) => Promise<SessionInfo>) {
    const s = await this.ensureSession()
    this.patch({ session: await f(s.id) })
  }

  setAutocommit = (on: boolean) => this.sessionOp((id) => this.api.setAutocommit(id, on))
  commit = () => this.sessionOp((id) => this.api.commit(id))
  rollback = () => this.sessionOp((id) => this.api.rollback(id))
  setSchema = (schema: string) => this.sessionOp((id) => this.api.setSchema(id, schema))
  reconnect = () => this.sessionOp((id) => this.api.reconnect(id))

  /** 关闭会话（有未提交的修改时按 commit 提交或回滚）并丢掉状态。 */
  async dispose(commit = false) {
    const s = this.rt().session
    this.socket?.close()
    this.socket = null
    useRuntime.getState().drop(this.tabId)
    if (s) await this.api.closeSession(s.id, commit).catch((e: unknown) => console.warn('关闭会话失败', e))
  }
}

const runners = new Map<string, Runner>()

export function runnerFor(api: Api, tabId: string): Runner {
  let r = runners.get(tabId)
  if (!r) {
    r = new Runner(api, tabId)
    runners.set(tabId, r)
  }
  return r
}

/** 关闭标签页：关闭会话、丢掉执行器。 */
export async function disposeRunner(tabId: string, commit = false) {
  const r = runners.get(tabId)
  runners.delete(tabId)
  if (r) await r.dispose(commit)
  else useRuntime.getState().drop(tabId)
}
