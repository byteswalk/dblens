// 模型的对话框（M4a 设计文档第 7 节）：打开、新建、从库反向生成。菜单、对象树、模型面板共用。

import { create } from 'zustand'

/** 反向生成的初始值：对象树中 schema 的"另存为模型…"带上连接和 schema。 */
export interface ReverseInit {
  profileId: string | null
  schema: string | null
}

interface ModelDialogs {
  open: boolean
  create: boolean
  reverse: ReverseInit | null
  openOpen(): void
  openCreate(): void
  openReverse(init?: ReverseInit): void
  close(): void
}

export const useModelDialogs = create<ModelDialogs>()((set) => ({
  open: false,
  create: false,
  reverse: null,
  openOpen: () => set({ open: true }),
  openCreate: () => set({ create: true }),
  openReverse: (init = { profileId: null, schema: null }) => set({ reverse: init }),
  close: () => set({ open: false, create: false, reverse: null }),
}))

/** 目录的最后一段（新建模型时作为默认名称）。 */
export function baseName(path: string): string {
  return path.replace(/[\\/]+$/, '').split(/[\\/]/).pop() ?? ''
}
