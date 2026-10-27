// 导出对话框的状态：结果面板和数据标签都可以打开它。

import { create } from 'zustand'

import type { ExportSource } from '../../shared/api'

export interface ExportDraft {
  profileId: string
  source: ExportSource
  /** 默认文件名的前半部分（对象名，查询结果为 export）。 */
  baseName: string
  /** INSERT 的默认目标表。 */
  table?: string
  /** 标签有未提交的事务：提示导出看不到这些修改。 */
  inTransaction: boolean
}

interface DialogState {
  draft: ExportDraft | null
  open(draft: ExportDraft): void
  close(): void
}

export const useExportDialog = create<DialogState>()((set) => ({
  draft: null,
  open: (draft) => set({ draft }),
  close: () => set({ draft: null }),
}))
