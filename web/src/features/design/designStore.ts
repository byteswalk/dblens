// 每个设计器标签的状态（不随标签保存）：原表、指纹、草稿、是否手动选择重建。关闭标签前用 hasDesignChanges 判断要不要确认放弃。

import { create } from 'zustand'

import type { DesignTableResult } from '../../shared/api'
import { type Draft, isDirty } from './logic'

export interface DesignState {
  /** 原表（新建表时为 null）。 */
  original: Draft | null
  fingerprint: string | null
  storage: DesignTableResult['storage'] | null
  draft: Draft
  /** 用户在预览中手动选择了重建。 */
  rebuild: boolean
}

interface DesignStore {
  tabs: Record<string, DesignState>
  set(tabId: string, state: DesignState): void
  /** 修改草稿。 */
  edit(tabId: string, fn: (d: Draft) => Draft): void
  setRebuild(tabId: string, on: boolean): void
  clear(tabId: string): void
}

export const useDesigns = create<DesignStore>()((set, get) => ({
  tabs: {},
  set: (tabId, state) => set({ tabs: { ...get().tabs, [tabId]: state } }),
  edit: (tabId, fn) => {
    const s = get().tabs[tabId]
    if (s) set({ tabs: { ...get().tabs, [tabId]: { ...s, draft: fn(s.draft) } } })
  },
  setRebuild: (tabId, on) => {
    const s = get().tabs[tabId]
    if (s) set({ tabs: { ...get().tabs, [tabId]: { ...s, rebuild: on } } })
  },
  clear: (tabId) => {
    const tabs = { ...get().tabs }
    delete tabs[tabId]
    set({ tabs })
  },
}))

export function hasDesignChanges(tabId: string): boolean {
  const s = useDesigns.getState().tabs[tabId]
  return !!s && isDirty(s.draft, s.original)
}
