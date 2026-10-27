// SQL 编辑器：Monaco。每个标签页一个模型（以标签 id 为路径），切换标签时保留各自的撤销历史和光标。
// 快捷键（M1 设计文档 1.3 节）注册为编辑器动作，焦点在编辑器里时由 Monaco 处理；错误标记来自执行结果。
// 补全与语法检查（M1b2）：模型绑定所在标签页的连接上下文，由 features/sql 中的 provider 和 checker 处理。

import Editor, { type OnMount } from '@monaco-editor/react'
import { useEffect, useRef } from 'react'
import { useTranslation } from 'react-i18next'

import { useSetting } from '../../shared/settings'
import { THEME_KEY, parseThemePref, resolveTheme, useSystemDark } from '../../shared/theme'
import { attachChecker } from '../sql/checker'
import { type EditorSqlContext, bindModel, registerSqlProviders } from '../sql/provider'
import { type EditorCommand, sendCommand } from './commands'
import { useCursor } from './cursor'
import type { ErrorMark } from './markers'
import { monaco } from './monaco'

export type MonacoEditor = Parameters<OnMount>[0]

registerSqlProviders()

const { KeyMod, KeyCode } = monaco
type CommandLabel = `editor.command.${'runCurrent' | 'runAll' | 'stop' | 'format' | 'explain' | 'favorite'}`
// label 是词条键，注册动作时按当前界面语言翻译
const ACTIONS: { id: EditorCommand; label: CommandLabel; keys: number[] }[] = [
  { id: 'run-current', label: 'editor.command.runCurrent', keys: [KeyMod.CtrlCmd | KeyCode.Enter] },
  { id: 'run-all', label: 'editor.command.runAll', keys: [KeyCode.F5] },
  { id: 'cancel', label: 'editor.command.stop', keys: [KeyMod.CtrlCmd | KeyCode.PauseBreak, KeyMod.CtrlCmd | KeyCode.Period] },
  { id: 'format', label: 'editor.command.format', keys: [KeyMod.Shift | KeyMod.Alt | KeyCode.KeyF] },
  { id: 'explain', label: 'editor.command.explain', keys: [] },
  // 覆盖 Monaco 默认的 Ctrl+D（选中下一个相同的词）
  { id: 'favorite', label: 'editor.command.favorite', keys: [KeyMod.CtrlCmd | KeyCode.KeyD] },
]

export interface SqlEditorProps {
  path: string
  language: string
  defaultValue: string
  marks: ErrorMark[]
  /** 补全用的上下文；连接已删除时为 null。 */
  sql: EditorSqlContext | null
  onChange(value: string): void
  onReady(editor: MonacoEditor | null): void
}

export default function SqlEditor({ path, language, defaultValue, marks, sql, onChange, onReady }: SqlEditorProps) {
  const { t, i18n } = useTranslation()
  const systemDark = useSystemDark()
  const theme = resolveTheme(parseThemePref(useSetting(THEME_KEY)), systemDark)
  const editorRef = useRef<MonacoEditor | null>(null)
  const sqlRef = useRef(sql)
  sqlRef.current = sql
  useEffect(() => bindModel(path, () => sqlRef.current), [path])
  const checker = useRef<ReturnType<typeof attachChecker> | null>(null)
  useEffect(() => () => checker.current?.dispose(), [])
  // 识别出变体（标准档 / 兼容档）、换了连接或界面语言时重新检查
  useEffect(() => checker.current?.recheck(), [sql?.driver, sql?.variant, i18n.language])

  const onMount: OnMount = (editor) => {
    editorRef.current = editor
    checker.current = attachChecker(editor)
    for (const a of ACTIONS) {
      editor.addAction({
        id: `dblens.${a.id}`,
        label: t(a.label),
        keybindings: a.keys,
        contextMenuGroupId: a.id === 'cancel' ? undefined : '0_dblens',
        run: () => sendCommand(a.id),
      })
    }
    editor.onDidChangeCursorPosition((e) =>
      useCursor.setState({ line: e.position.lineNumber, column: e.position.column }),
    )
    onReady(editor)
    editor.focus()
  }
  useEffect(() => () => onReady(null), [onReady])
  // 切换或新建标签后直接可以输入
  useEffect(() => editorRef.current?.focus(), [path])

  // 错误标记：执行结果变化时整体替换
  useEffect(() => {
    const model = editorRef.current?.getModel()
    if (!model || model.uri.path !== monaco.Uri.parse(path).path) return
    monaco.editor.setModelMarkers(
      model,
      'dblens',
      marks.map((m) => {
        const s = model.getPositionAt(m.start)
        const e = model.getPositionAt(m.end)
        return {
          severity: monaco.MarkerSeverity.Error,
          message: m.message,
          startLineNumber: s.lineNumber,
          startColumn: s.column,
          endLineNumber: e.lineNumber,
          endColumn: e.column,
        }
      }),
    )
  }, [marks, path])

  return (
    <Editor
      path={path}
      // 切换标签时保留模型（撤销历史）；关闭标签时由工作区删除
      keepCurrentModel
      language={language}
      defaultValue={defaultValue}
      theme={theme === 'dark' ? 'dblens-dark' : 'dblens-light'}
      onMount={onMount}
      onChange={(v) => onChange(v ?? '')}
      options={{
        fontFamily: "'JetBrains Mono', 'Cascadia Mono', Consolas, 'Noto Sans Mono CJK SC', monospace",
        fontSize: 14,
        lineHeight: 22,
        minimap: { enabled: false },
        scrollBeyondLastLine: false,
        automaticLayout: true,
        tabSize: 2,
        fixedOverflowWidgets: true,
        renderLineHighlight: 'line',
        // 只用 SQL 补全，不补文中出现过的词
        wordBasedSuggestions: 'off',
      }}
    />
  )
}

/** 删除已关闭标签页的模型。 */
export function disposeModel(path: string) {
  monaco.editor.getModel(monaco.Uri.parse(path))?.dispose()
}
