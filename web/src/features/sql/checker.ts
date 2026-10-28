// 编辑时的语法检查（M1b2 设计文档第 5、7 节）：
// - 编辑后 400 ms 把整个脚本交给 worker 检查，结果标在编辑器里（与执行出错的标记来源不同，互不覆盖）；
// - 设置关闭、连接已删除、解析器不可用时不检查；脚本超过 1 MB 时不检查；
// - 单次超过 2 秒丢弃结果，连续三次超时后该标签页停止检查（状态栏提示）。

import { driverLabel } from '../connections/form'
import { variantName } from '../editor/dialect'
import { monaco } from '../editor/monaco'
import { currentLang } from '../../shared/i18n'
import { useSettings } from '../../shared/settings'
import { sqlDialect } from './dialects'
import { syntaxMarks } from './diagnostics'
import { contextOf, pathOf } from './provider'
import { sqlClient } from './service'
import { NO_SYNTAX, SYNTAX_KEY, type SyntaxState, syntaxEnabled, useSyntax } from './syntax'

const OWNER = 'dblens-syntax'
export const DEBOUNCE_MS = 400
export const MAX_CHARS = 1_000_000
const MAX_TIMEOUTS = 3

type Editor = monaco.editor.IStandaloneCodeEditor
type Model = monaco.editor.ITextModel

/** 每个模型连续超时的次数；达到上限后不再检查。 */
const timeouts = new WeakMap<Model, number>()

async function check(model: Model) {
  const path = pathOf(model)
  if (!path || model.isDisposed()) return
  const report = (s: SyntaxState) => useSyntax.getState().set(path, s)
  const clear = (s: SyntaxState) => {
    monaco.editor.setModelMarkers(model, OWNER, [])
    report(s)
  }
  const ctx = contextOf(model)
  if (!ctx || !syntaxEnabled(useSettings.getState().values[SYNTAX_KEY])) return clear(NO_SYNTAX)
  if ((timeouts.get(model) ?? 0) >= MAX_TIMEOUTS) return clear({ ...NO_SYNTAX, status: 'stopped' })
  const text = model.getValue()
  if (text.length > MAX_CHARS) return clear({ ...NO_SYNTAX, status: 'skipped' })
  const d = sqlDialect(ctx.driver, ctx.variant, ctx.compat)
  if (sqlClient.unavailable(d.dialect)) return clear(NO_SYNTAX)

  const version = model.getVersionId()
  const r = await sqlClient.validate(d.dialect, text, currentLang() === 'zh-CN' ? 'zh_CN' : 'en_US')
  // 检查期间又改过：结果作废（改动会触发新的检查）
  if (model.isDisposed() || model.getVersionId() !== version) return
  if (!r.ok) {
    if (r.reason === 'timeout') {
      const n = (timeouts.get(model) ?? 0) + 1
      timeouts.set(model, n)
      if (n >= MAX_TIMEOUTS) clear({ ...NO_SYNTAX, status: 'stopped' })
    } else clear(NO_SYNTAX)
    return
  }
  timeouts.delete(model)
  const name = variantName(ctx.variant) ?? driverLabel(ctx.driver)
  const marks = syntaxMarks(r.value, d, name)
  monaco.editor.setModelMarkers(
    model,
    OWNER,
    marks.map((m) => ({ ...m, severity: m.severity === 'error' ? monaco.MarkerSeverity.Error : monaco.MarkerSeverity.Warning })),
  )
  const first = marks[0] ? { line: marks[0].startLineNumber, column: marks[0].startColumn } : null
  report({ status: 'ok', count: marks.length, first })
}

/**
 * 给编辑器装上语法检查：内容变化、切换模型时检查当前模型。
 * 返回 recheck（上下文、设置、语言变化时调用）和 dispose。
 */
export function attachChecker(editor: Editor) {
  let timer: ReturnType<typeof setTimeout> | undefined
  const schedule = (delay = DEBOUNCE_MS) => {
    clearTimeout(timer)
    timer = setTimeout(() => {
      const model = editor.getModel()
      if (model) void check(model)
    }, delay)
  }
  const subs = [editor.onDidChangeModelContent(() => schedule()), editor.onDidChangeModel(() => schedule(0))]
  const unsubscribe = useSettings.subscribe((s, prev) => {
    if (s.values[SYNTAX_KEY] !== prev.values[SYNTAX_KEY]) schedule(0)
  })
  schedule(0)
  return {
    recheck: () => schedule(0),
    dispose: () => {
      clearTimeout(timer)
      unsubscribe()
      for (const s of subs) s.dispose()
    },
  }
}
