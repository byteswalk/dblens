// 每个 ER 图标签的编辑状态（不随标签保存，切换标签时保留）：位置、显示方式、主题图的表、指纹、是否有未保存的改动。
// 另有 ER 图与设计器之间的通知：在 ER 图中点"新建表"打开设计器，设计器保存新表后把表加进这张主题图。
// 关闭标签前用 hasErChanges 判断要不要确认放弃。

import { create } from 'zustand'

import type { ErDisplay, ErPosition } from '../../shared/api'

export interface ErTabState {
  positions: Record<string, ErPosition>
  display: ErDisplay
  tables: 'all' | string[]
  fingerprint: string | null
  dirty: boolean
  /** 读到的文件版本（`标签:指纹`）：没有未保存的改动时，文件变了就重新取。 */
  loadedKey: string
}

interface ErTabs {
  tabs: Record<string, ErTabState>
  /** 设计器标签 id → 等着它保存新表的 ER 图标签。 */
  waiting: Record<string, string>
  set(tabId: string, state: ErTabState): void
  patch(tabId: string, patch: Partial<ErTabState>): void
  clear(tabId: string): void
  wait(designTab: string, erTab: string): void
  /** 设计器保存了新表：有主题图在等时把表加进去。 */
  tableCreated(designTab: string, name: string): void
}

export const useErTabs = create<ErTabs>()((set, get) => ({
  tabs: {},
  waiting: {},
  set: (tabId, state) => set({ tabs: { ...get().tabs, [tabId]: state } }),
  patch: (tabId, patch) => {
    const s = get().tabs[tabId]
    if (s) set({ tabs: { ...get().tabs, [tabId]: { ...s, ...patch } } })
  },
  clear: (tabId) => {
    const tabs = { ...get().tabs }
    delete tabs[tabId]
    set({ tabs })
  },
  wait: (designTab, erTab) => set({ waiting: { ...get().waiting, [designTab]: erTab } }),
  tableCreated: (designTab, name) => {
    const erTab = get().waiting[designTab]
    if (!erTab) return
    const waiting = { ...get().waiting }
    delete waiting[designTab]
    set({ waiting })
    const s = get().tabs[erTab]
    if (s && s.tables !== 'all' && !s.tables.includes(name)) get().patch(erTab, { tables: [...s.tables, name], dirty: true })
  },
}))

export function hasErChanges(tabId: string): boolean {
  return !!useErTabs.getState().tabs[tabId]?.dirty
}
