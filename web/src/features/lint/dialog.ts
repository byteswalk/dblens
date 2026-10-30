// 发起检查对话框的打开状态：对象树和模型面板共用。

import { create } from 'zustand'

import type { LintSource } from '../../shared/api'

interface LintStartDialog {
  source: LintSource | null
  open(source: LintSource): void
  close(): void
}

export const useLintStart = create<LintStartDialog>()((set) => ({
  source: null,
  open: (source) => set({ source }),
  close: () => set({ source: null }),
}))
