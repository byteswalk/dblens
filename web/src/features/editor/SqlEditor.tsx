// SQL 编辑器：Monaco。每个标签页一个模型（以标签 id 为路径），切换标签时保留各自的撤销历史和光标。
// 快捷键（M1 设计文档 1.3 节）注册为编辑器动作，焦点在编辑器里时由 Monaco 处理；错误标记来自执行结果。

import Editor, { type OnMount } from '@monaco-editor/react'
import { useEffect, useRef } from 'react'

import { useSetting } from '../../shared/settings'
import { THEME_KEY, parseThemePref, resolveTheme, useSystemDark } from '../../shared/theme'
import { type EditorCommand, sendCommand } from './commands'
import { useCursor } from './cursor'
import type { ErrorMark } from './markers'
import { monaco } from './monaco'

export type MonacoEditor = Parameters<OnMount>[0]

const { KeyMod, KeyCode } = monaco
const ACTIONS: { id: EditorCommand; label: string; keys: number[] }[] = [
  { id: 'run-current', label: '执行当前语句', keys: [KeyMod.CtrlCmd | KeyCode.Enter] },
  { id: 'run-all', label: '执行全部', keys: [KeyCode.F5] },
  { id: 'cancel', label: '停止', keys: [KeyMod.CtrlCmd | KeyCode.PauseBreak, KeyMod.CtrlCmd | KeyCode.Period] },
  { id: 'format', label: '格式化', keys: [KeyMod.Shift | KeyMod.Alt | KeyCode.KeyF] },
  { id: 'explain', label: '执行计划', keys: [] },
]

export interface SqlEditorProps {
  path: string
  language: string
  defaultValue: string
  marks: ErrorMark[]
  onChange(value: string): void
  onReady(editor: MonacoEditor | null): void
}

export default function SqlEditor({ path, language, defaultValue, marks, onChange, onReady }: SqlEditorProps) {
  const systemDark = useSystemDark()
  const theme = resolveTheme(parseThemePref(useSetting(THEME_KEY)), systemDark)
  const editorRef = useRef<MonacoEditor | null>(null)

  const onMount: OnMount = (editor) => {
    editorRef.current = editor
    for (const a of ACTIONS) {
      editor.addAction({
        id: `dblens.${a.id}`,
        label: a.label,
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
        fontSize: 13,
        lineHeight: 20,
        minimap: { enabled: false },
        scrollBeyondLastLine: false,
        automaticLayout: true,
        tabSize: 2,
        fixedOverflowWidgets: true,
        renderLineHighlight: 'line',
      }}
    />
  )
}

/** 删除已关闭标签页的模型。 */
export function disposeModel(path: string) {
  monaco.editor.getModel(monaco.Uri.parse(path))?.dispose()
}
