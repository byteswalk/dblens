// ER 图与设计器之间的通知：在 ER 图中点"新建表"打开设计器，设计器保存新表后通知这张图把表加进来（主题图）。

import { create } from 'zustand'

export interface CreatedTable {
  /** 模型目录。 */
  path: string
  schema: string
  name: string
  /** 等着加表的 ER 图标签。 */
  erTab: string
}

interface ErEvents {
  /** 设计器标签 id → 等着它保存新表的 ER 图标签。 */
  waiting: Record<string, string>
  created: CreatedTable | null
  wait(designTab: string, erTab: string): void
  /** 设计器保存了新表：有 ER 图在等时发出通知。 */
  tableCreated(designTab: string, path: string, schema: string, name: string): void
}

export const useErEvents = create<ErEvents>()((set, get) => ({
  waiting: {},
  created: null,
  wait: (designTab, erTab) => set({ waiting: { ...get().waiting, [designTab]: erTab } }),
  tableCreated: (designTab, path, schema, name) => {
    const erTab = get().waiting[designTab]
    if (!erTab) return
    const waiting = { ...get().waiting }
    delete waiting[designTab]
    set({ waiting, created: { path, schema, name, erTab } })
  },
}))
