// 会话 WebSocket 执行协议（docs/notes/m0-4b-api.md、docs/notes/m1b1-editor.md）：发送 exec / fetch_more / cancel，
// 把服务端事件归并成界面要显示的状态。归并是纯函数，单独测试；ExecSocket 只负责收发。

import type { ErrorBody } from './api'
import { t } from './i18n'

/** 单元格值：{"t": 类型, "v": 值}，Null 没有 v。 */
export interface CellValue {
  t: string
  v?: unknown
}

export interface ColumnMeta {
  name: string
  type_name: string
  nullable: boolean | null
}

export type ServerEvent =
  | { type: 'statement_start'; index: number; total: number; start: number; end: number; info: { keyword: string } }
  | { type: 'result_start'; columns: ColumnMeta[] }
  | { type: 'rows'; rows: CellValue[][] }
  | { type: 'result_end' }
  | { type: 'affected'; count: number }
  | { type: 'message'; text: string }
  | { type: 'statement_end'; index: number; elapsed_ms: number }
  | { type: 'confirm_required'; index: number; start: number; end: number; reason: string }
  | { type: 'failed'; index: number; error: ErrorBody; offset?: number }
  | { type: 'done' }
  | { type: 'error'; error: ErrorBody }

export interface ResultSet {
  columns: ColumnMeta[]
  rows: CellValue[][]
  /** 结果集已取完。 */
  complete: boolean
}

export interface StatementState {
  index: number
  /** 是这次提交的最后一条语句。前面语句的结果要取完，后面的语句才会执行。 */
  last: boolean
  /** 语句在提交的 SQL 中的 UTF-16 偏移，可直接用于 String.prototype.slice。 */
  start: number
  end: number
  keyword: string
  results: ResultSet[]
  affected: number | null
  messages: string[]
  elapsedMs: number | null
  error: ErrorBody | null
  /** 数据库报告的出错位置（UTF-16 偏移）；没有时为 null。 */
  errorOffset: number | null
}

export interface ExecState {
  statements: StatementState[]
  /** 一批装满了 max_rows 行，服务端在等 fetch_more。 */
  waitingForMore: boolean
  done: boolean
  /** 第 `index` 条语句需要确认，脚本停在这里（这条还没有开始）。`start`/`end` 是它的 UTF-16 范围。 */
  confirm: { index: number; start: number; end: number; reason: string } | null
  /** 协议层面的错误（消息无法解析、上一段还没执行完等）。 */
  protocolError: ErrorBody | null
}

export const initialExec: ExecState = {
  statements: [],
  waitingForMore: false,
  done: false,
  confirm: null,
  protocolError: null,
}

function updateLast(state: ExecState, f: (s: StatementState) => StatementState): ExecState {
  const last = state.statements.length - 1
  if (last < 0) return state
  return { ...state, statements: state.statements.map((s, i) => (i === last ? f(s) : s)) }
}

function updateLastResult(s: StatementState, f: (r: ResultSet) => ResultSet): StatementState {
  const last = s.results.length - 1
  if (last < 0) return s
  return { ...s, results: s.results.map((r, i) => (i === last ? f(r) : r)) }
}

/** 把一个服务端事件归并进状态。`maxRows` 是本次请求的每批行数，用来判断服务端是否在等 fetch_more。 */
export function reduceExec(state: ExecState, ev: ServerEvent, maxRows: number): ExecState {
  switch (ev.type) {
    case 'statement_start':
      return {
        ...state,
        statements: [
          ...state.statements,
          {
            index: ev.index,
            last: ev.index + 1 >= ev.total,
            start: ev.start,
            end: ev.end,
            keyword: ev.info.keyword,
            results: [],
            affected: null,
            messages: [],
            elapsedMs: null,
            error: null,
            errorOffset: null,
          },
        ],
      }
    case 'result_start':
      return updateLast(state, (s) => ({ ...s, results: [...s.results, { columns: ev.columns, rows: [], complete: false }] }))
    case 'rows':
      return {
        ...updateLast(state, (s) => updateLastResult(s, (r) => ({ ...r, rows: [...r.rows, ...ev.rows] }))),
        waitingForMore: ev.rows.length >= maxRows,
      }
    case 'result_end':
      return { ...updateLast(state, (s) => updateLastResult(s, (r) => ({ ...r, complete: true }))), waitingForMore: false }
    case 'affected':
      return updateLast(state, (s) => ({ ...s, affected: ev.count }))
    case 'message':
      return updateLast(state, (s) => ({ ...s, messages: [...s.messages, ev.text] }))
    case 'statement_end':
      return updateLast(state, (s) => ({ ...s, elapsedMs: ev.elapsed_ms }))
    case 'confirm_required':
      return { ...state, confirm: { index: ev.index, start: ev.start, end: ev.end, reason: ev.reason } }
    case 'failed':
      return {
        ...updateLast(state, (s) => ({ ...s, error: ev.error, errorOffset: ev.offset ?? null })),
        waitingForMore: false,
      }
    case 'done':
      return { ...state, done: true, waitingForMore: false }
    case 'error':
      return { ...state, protocolError: ev.error, waitingForMore: false }
  }
}

export const DEFAULT_MAX_ROWS = 500

export interface ExecOptions {
  /** 已确认：需要确认的语句直接执行。 */
  confirmed?: boolean
  /** 某条语句失败后继续执行后面的。 */
  continueOnError?: boolean
  /** 只执行这个 UTF-16 偏移所在的那条语句。 */
  cursor?: number
  /** 只生成执行计划。 */
  explain?: boolean
  maxRows?: number
}

/** 一个会话的执行连接。onEvent 收到服务端的每个事件；连接断开时服务端会取消正在执行的语句。 */
export class ExecSocket {
  private ws: WebSocket
  private ready: Promise<void>

  constructor(url: string, onEvent: (ev: ServerEvent) => void, onClose: () => void) {
    this.ws = new WebSocket(url)
    this.ready = new Promise((resolve, reject) => {
      this.ws.addEventListener('open', () => resolve(), { once: true })
      this.ws.addEventListener('error', () => reject(new Error(t('app.exec.socketFailed'))), { once: true })
    })
    this.ws.addEventListener('message', (m) => onEvent(JSON.parse(m.data as string) as ServerEvent))
    this.ws.addEventListener('close', onClose)
  }

  private async send(msg: object) {
    await this.ready
    this.ws.send(JSON.stringify(msg))
  }

  exec(sql: string, o: ExecOptions = {}) {
    return this.send({
      type: 'exec',
      sql,
      confirmed: o.confirmed ?? false,
      continue_on_error: o.continueOnError ?? false,
      cursor: o.cursor ?? null,
      explain: o.explain ?? false,
      max_rows: o.maxRows ?? DEFAULT_MAX_ROWS,
    })
  }

  fetchMore(maxRows = DEFAULT_MAX_ROWS) {
    return this.send({ type: 'fetch_more', max_rows: maxRows })
  }

  cancel() {
    return this.send({ type: 'cancel' })
  }

  close() {
    this.ws.close()
  }
}

/** 单元格的显示文本。 */
export function cellText(c: CellValue): string {
  if (c.t === 'null') return 'NULL'
  if (c.t === 'lob') {
    const lob = c.v as { preview: string; total_len: number; truncated: boolean; binary: boolean }
    if (!lob.truncated) return lob.preview
    const v = { preview: lob.preview, total: lob.total_len }
    return lob.binary ? t('app.exec.lobTruncatedBinary', v) : t('app.exec.lobTruncated', v)
  }
  if (c.t === 'other') return (c.v as { text: string }).text
  if (typeof c.v === 'object') return JSON.stringify(c.v)
  return String(c.v)
}
