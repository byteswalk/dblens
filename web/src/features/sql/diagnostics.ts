// 语法问题 → 编辑器标记（M1b2 设计文档第 5 节）：标准库标为错误（红），兼容库标为警告（黄）并说明按哪种语法检查。

import { t } from '../../shared/i18n'
import type { SqlDialect } from './dialects'
import type { ParseIssue } from './types'

export interface SyntaxMark {
  severity: 'error' | 'warning'
  message: string
  startLineNumber: number
  startColumn: number
  endLineNumber: number
  endColumn: number
}

/** name：数据库的名称（变体名或驱动名），兼容档的提示中使用。 */
export function syntaxMarks(issues: ParseIssue[], d: SqlDialect, name: string): SyntaxMark[] {
  return issues.map((i) => {
    let message = i.message ?? (i.incomplete ? t('editor.syntax.incomplete') : t('editor.syntax.near', { near: i.near ?? '' }))
    if (!d.strict) message += t('editor.syntax.compatHint', { grammar: d.grammar, name })
    return {
      severity: d.strict ? 'error' : 'warning',
      message,
      startLineNumber: i.startLine,
      startColumn: i.startColumn,
      endLineNumber: i.endLine,
      // 至少标出一个字符
      endColumn: i.endLine === i.startLine ? Math.max(i.endColumn, i.startColumn + 1) : i.endColumn,
    }
  })
}
