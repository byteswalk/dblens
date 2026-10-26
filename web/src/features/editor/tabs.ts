// 查询标签页（M1 设计文档 1 节）：每个标签绑定一个连接，有自己的 SQL。打开的标签和其中的 SQL 存进设置
// （ui.tabs），下次启动时恢复；会话不恢复，第一次执行时再打开。

import { create } from 'zustand'

import { useSettings } from '../../shared/settings'

export const TABS_KEY = 'ui.tabs'

export interface QueryTab {
  id: string
  /** 「查询1」这样的标题。 */
  title: string
  profileId: string
  sql: string
}

interface Persisted {
  tabs: QueryTab[]
  active: string | null
  /** 下一个新标签的编号。 */
  seq: number
}

/** 解析保存的标签；格式不对时丢弃（界面设置不值得报错）。 */
export function parseTabs(raw: string | undefined): Persisted {
  const empty: Persisted = { tabs: [], active: null, seq: 1 }
  if (!raw) return empty
  try {
    const v = JSON.parse(raw) as Partial<Persisted>
    const tabs = (Array.isArray(v.tabs) ? v.tabs : []).filter(
      (t): t is QueryTab =>
        typeof t?.id === 'string' &&
        typeof t.title === 'string' &&
        typeof t.profileId === 'string' &&
        typeof t.sql === 'string',
    )
    const active = tabs.some((t) => t.id === v.active) ? (v.active as string) : (tabs[0]?.id ?? null)
    return { tabs, active, seq: typeof v.seq === 'number' && v.seq > 0 ? v.seq : tabs.length + 1 }
  } catch {
    return empty
  }
}

interface TabsState extends Persisted {
  /** 新建标签并切换过去，返回它的 id。 */
  open(profileId: string, sql?: string): string
  close(id: string): void
  activate(id: string): void
  setSql(id: string, sql: string): void
  setProfile(id: string, profileId: string): void
  restore(raw: string | undefined): void
}

let ids = 0
const newId = () => `t${Date.now().toString(36)}${(ids++).toString(36)}`

export const useTabs = create<TabsState>()((set, get) => ({
  tabs: [],
  active: null,
  seq: 1,
  open: (profileId, sql = '') => {
    const { tabs, seq } = get()
    const tab = { id: newId(), title: `查询${seq}`, profileId, sql }
    set({ tabs: [...tabs, tab], active: tab.id, seq: seq + 1 })
    return tab.id
  },
  close: (id) => {
    const { tabs, active } = get()
    const i = tabs.findIndex((t) => t.id === id)
    if (i < 0) return
    const rest = tabs.filter((t) => t.id !== id)
    // 关闭当前标签时切到右边的，没有就左边的
    const next = active === id ? (rest[i] ?? rest[i - 1])?.id ?? null : active
    set({ tabs: rest, active: next })
  },
  activate: (active) => set({ active }),
  setSql: (id, sql) => set({ tabs: get().tabs.map((t) => (t.id === id ? { ...t, sql } : t)) }),
  setProfile: (id, profileId) => set({ tabs: get().tabs.map((t) => (t.id === id ? { ...t, profileId } : t)) }),
  restore: (raw) => set(parseTabs(raw)),
}))

let stopPersist: (() => void) | null = null

/** 标签有变化时延迟保存到设置（输入 SQL 时不必每个字都保存）。重复调用不会重复订阅。返回取消订阅的函数。 */
export function persistTabs(delayMs = 800): () => void {
  if (stopPersist) return stopPersist
  let timer: ReturnType<typeof setTimeout> | undefined
  const unsubscribe = useTabs.subscribe((s, prev) => {
    if (s.tabs === prev.tabs && s.active === prev.active) return
    clearTimeout(timer)
    timer = setTimeout(() => {
      const { tabs, active, seq } = useTabs.getState()
      useSettings.getState().set(TABS_KEY, JSON.stringify({ tabs, active, seq }))
    }, delayMs)
  })
  stopPersist = () => {
    clearTimeout(timer)
    unsubscribe()
    stopPersist = null
  }
  return stopPersist
}
