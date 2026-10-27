// 每个数据标签的编辑状态（不随标签保存）。关闭、刷新等操作前用 hasPendingEdits 判断要不要确认放弃。

import { create } from 'zustand'

import { type EditState, changeCount, emptyEdit } from './edit'

interface EditStore {
  tabs: Record<string, EditState>
  /** 提交失败的行（表格中的行号），标红显示；改动后清除。 */
  failed: Record<string, number | null>
  set(tabId: string, state: EditState): void
  setFailed(tabId: string, row: number | null): void
  clear(tabId: string): void
}

export const useEdits = create<EditStore>()((set, get) => ({
  tabs: {},
  failed: {},
  set: (tabId, state) => set({ tabs: { ...get().tabs, [tabId]: state }, failed: { ...get().failed, [tabId]: null } }),
  setFailed: (tabId, row) => set({ failed: { ...get().failed, [tabId]: row } }),
  clear: (tabId) => {
    const tabs = { ...get().tabs }
    const failed = { ...get().failed }
    delete tabs[tabId]
    delete failed[tabId]
    set({ tabs, failed })
  },
}))

export function editsOf(tabId: string): EditState {
  return useEdits.getState().tabs[tabId] ?? emptyEdit
}

export function hasPendingEdits(tabId: string): boolean {
  return changeCount(editsOf(tabId)) > 0
}
