// 语法检查的设置和每个标签页的结果（状态栏显示）。单独放一个文件，状态栏和设置不必引入 Monaco。

import { create } from 'zustand'

/** 设置项：编辑时检查语法，默认开；值为 off 时关闭。 */
export const SYNTAX_KEY = 'ui.editor.syntax-check'

export const syntaxEnabled = (raw: string | undefined) => raw !== 'off'

export interface SyntaxState {
  /** ok：已检查；stopped：连续超时，已停止；skipped：脚本过大，未检查；off：没有检查（关闭或解析器不可用）。 */
  status: 'ok' | 'stopped' | 'skipped' | 'off'
  count: number
  /** 第一处问题的位置，状态栏点击时跳过去。 */
  first: { line: number; column: number } | null
}

export const NO_SYNTAX: SyntaxState = { status: 'off', count: 0, first: null }

interface SyntaxStore {
  tabs: Record<string, SyntaxState>
  set(tabId: string, s: SyntaxState): void
}

export const useSyntax = create<SyntaxStore>()((set, get) => ({
  tabs: {},
  set: (tabId, s) => set({ tabs: { ...get().tabs, [tabId]: s } }),
}))
