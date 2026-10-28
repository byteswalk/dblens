// Monaco 的 SQL 补全（M1b2 设计文档第 4 节）。三种语言（mysql、pgsql、plsql）共用一个补全提供者；
// 每个编辑器模型绑定它所在标签页的上下文（连接、变体、当前 schema、元数据），提供者按模型找到上下文。

import type { Compat, DriverId } from '../../shared/api'
import { monaco } from '../editor/monaco'
import { type ItemKind, completionItems, lightSuggest } from './completion'
import { sqlDialect } from './dialects'
import type { MetaReader } from './metadata'
import { sqlClient } from './service'

export interface EditorSqlContext {
  driver: DriverId
  variant: string | null | undefined
  /** 会话的兼容模式；没有会话时为空。 */
  compat?: Compat | null
  /** 当前 schema：会话的，没有会话时按连接配置推断。 */
  schema: string | null
  meta: MetaReader
}

interface Binding {
  path: string
  get: () => EditorSqlContext | null
}

const bindings = new Map<string, Binding>()

/** 把编辑器模型（按 path，即标签页 id）绑定到上下文；返回解绑函数。 */
export function bindModel(path: string, get: () => EditorSqlContext | null): () => void {
  const key = monaco.Uri.parse(path).toString()
  const b = { path, get }
  bindings.set(key, b)
  return () => {
    if (bindings.get(key) === b) bindings.delete(key)
  }
}

export function contextOf(model: monaco.editor.ITextModel): EditorSqlContext | null {
  return bindings.get(model.uri.toString())?.get() ?? null
}

/** 模型所属的标签页 id。 */
export function pathOf(model: monaco.editor.ITextModel): string | null {
  return bindings.get(model.uri.toString())?.path ?? null
}

const { CompletionItemKind: K } = monaco.languages
const KIND: Record<ItemKind, monaco.languages.CompletionItemKind> = {
  keyword: K.Keyword,
  schema: K.Module,
  table: K.Struct,
  view: K.Interface,
  synonym: K.Reference,
  sequence: K.Value,
  column: K.Field,
  function: K.Function,
}

const provider: monaco.languages.CompletionItemProvider = {
  triggerCharacters: ['.'],
  async provideCompletionItems(model, position) {
    const ctx = contextOf(model)
    if (!ctx) return { suggestions: [] }
    const { dialect } = sqlDialect(ctx.driver, ctx.variant, ctx.compat)
    const text = model.getValue()
    const offset = model.getOffsetAt(position)
    // 解析器不可用或超时：退回轻量补全
    const r = sqlClient.unavailable(dialect) ? null : await sqlClient.suggest(dialect, text, offset)
    const suggest = r?.ok ? r.value : lightSuggest(text, offset)
    const items = await completionItems({ suggest, dialect, schema: ctx.schema, meta: ctx.meta })
    const word = model.getWordUntilPosition(position)
    const range = new monaco.Range(position.lineNumber, word.startColumn, position.lineNumber, word.endColumn)
    return {
      suggestions: items.map((i) => ({
        label: i.label,
        kind: KIND[i.kind],
        detail: i.detail,
        insertText: i.insertText,
        sortText: i.sortText,
        range,
      })),
    }
  },
}

let registered = false

/** 注册补全提供者（重复调用无效果）。 */
export function registerSqlProviders() {
  if (registered) return
  registered = true
  for (const lang of ['mysql', 'pgsql', 'plsql']) monaco.languages.registerCompletionItemProvider(lang, provider)
}
