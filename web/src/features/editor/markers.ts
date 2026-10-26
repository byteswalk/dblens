// 编辑器中的错误标记（设计稿第 2 部分："波浪线：数据库返回的错误位置"）。
// 数据库报告了出错位置（PG、Oracle）时标出该位置的一个词，否则标出整条语句。取消不算错误。

import { errorText } from '../../shared/api'
import type { ExecState } from '../../shared/exec'

export interface ErrorMark {
  /** 编辑器全文中的 UTF-16 偏移。 */
  start: number
  end: number
  message: string
}

const WORD = /^[\p{L}\p{N}_$#.`"]+/u

export function errorMarks(exec: ExecState, text: string): ErrorMark[] {
  const out: ErrorMark[] = []
  for (const s of exec.statements) {
    if (!s.error || s.error.kind === 'cancelled') continue
    const message = errorText(s.error)
    if (s.errorOffset !== null && s.errorOffset < text.length) {
      const word = WORD.exec(text.slice(s.errorOffset))?.[0].length ?? 1
      out.push({ start: s.errorOffset, end: s.errorOffset + word, message })
    } else {
      out.push({ start: s.start, end: s.end, message })
    }
  }
  return out
}
