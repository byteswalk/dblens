// 导出文档对话框的打开状态：模型面板、对象树、ER 图共用。

import { create } from 'zustand'

import type { DocTarget } from './options'

interface DocExportDialog {
  target: DocTarget | null
  open(target: DocTarget): void
  close(): void
}

export const useDocExport = create<DocExportDialog>()((set) => ({
  target: null,
  open: (target) => set({ target }),
  close: () => set({ target: null }),
}))
