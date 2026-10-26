// 主线程与 SQL 语言 worker 之间的消息。

import type { Dialect, ParseIssue, ParserLocale, StatementRange, Suggest } from './types'

export type Request =
  | { id: number; op: 'load'; dialect: Dialect }
  | { id: number; op: 'validate'; dialect: Dialect; sql: string; locale: ParserLocale }
  | { id: number; op: 'suggest'; dialect: Dialect; sql: string; offset: number }
  | { id: number; op: 'statement'; dialect: Dialect; sql: string; offset: number }

/** 去掉 id 的请求（按各个分支分别去掉）。 */
export type RequestBody = Request extends infer R ? (R extends Request ? Omit<R, 'id'> : never) : never

export type Response =
  | { id: number; ok: true; result: ParseIssue[] | Suggest | StatementRange | null }
  | { id: number; ok: false; error: string }
