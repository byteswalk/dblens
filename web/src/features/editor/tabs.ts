// 查询标签页（M1 设计文档 1 节）：每个标签绑定一个连接，有自己的 SQL。打开的标签和其中的 SQL 存进设置
// （ui.tabs），下次启动时恢复；会话不恢复，第一次执行时再打开。
// 数据标签（M1c2 设计文档第 5 节）：带 data 的标签，只读地浏览一张表 / 视图 / 物化视图，同一对象只开一个。
// 设计器标签（M3 设计文档 7.1 节）：带 design 的标签，设计一张已有的表（同一张表只开一个）或新建表；草稿不保存。

import { create } from 'zustand'

import type { TargetKind } from '../../shared/api'
import { t } from '../../shared/i18n'
import { useSettings } from '../../shared/settings'

export const TABS_KEY = 'ui.tabs'

/** 数据标签浏览的对象，以及当前的过滤条件和排序（一并保存）。 */
export interface DataTarget {
  schema: string
  object: string
  objectKind: TargetKind
  /** WHERE 后的条件；空表示不过滤。 */
  filter?: string
  order?: { column: string; desc: boolean } | null
}

/** 设计器标签设计的表：`table` 为 null 时是新建表（`schema` 为空串时由用户在"选项"中选）。 */
export interface DesignTarget {
  schema: string
  table: string | null
}

export interface QueryTab {
  id: string
  /** 「查询1」这样的标题：新建时按界面语言生成，之后是数据，不随语言变化。数据标签为对象名。 */
  title: string
  profileId: string
  sql: string
  /** 有值时是数据标签。 */
  data?: DataTarget
  /** 有值时是设计器标签。 */
  design?: DesignTarget
}

const TARGET_KINDS: readonly unknown[] = ['table', 'view', 'materialized_view'] satisfies TargetKind[]

function isDataTarget(v: unknown): v is DataTarget {
  const d = v as DataTarget | null
  const o = d?.order
  return (
    typeof d?.schema === 'string' &&
    typeof d.object === 'string' &&
    TARGET_KINDS.includes(d.objectKind) &&
    (d.filter === undefined || typeof d.filter === 'string') &&
    (o === undefined || o === null || (typeof o.column === 'string' && typeof o.desc === 'boolean'))
  )
}

function isDesignTarget(v: unknown): v is DesignTarget {
  const d = v as DesignTarget | null
  return typeof d?.schema === 'string' && (d.table === null || typeof d.table === 'string')
}

/** 是否同一个对象的数据标签。 */
export function sameData(tab: QueryTab, profileId: string, d: DataTarget): boolean {
  return (
    tab.profileId === profileId &&
    tab.data?.schema === d.schema &&
    tab.data.object === d.object &&
    tab.data.objectKind === d.objectKind
  )
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
        typeof t.sql === 'string' &&
        (t.data === undefined || isDataTarget(t.data)) &&
        (t.design === undefined || isDesignTarget(t.design)),
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
  /** 打开对象的数据标签：已经打开过时切换过去。返回标签的 id。 */
  openData(profileId: string, target: DataTarget): string
  /** 打开表设计器：已有的表已经打开过时切换过去；新建表每次开一个新标签。返回标签的 id。 */
  openDesign(profileId: string, target: DesignTarget): string
  /** 新建的表保存后，标签转为设计这张表。 */
  setDesign(id: string, target: DesignTarget): void
  /** 修改数据标签的过滤条件和排序。 */
  setData(id: string, patch: Pick<DataTarget, 'filter' | 'order'>): void
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
    const tab = { id: newId(), title: t('editor.tab.untitled', { n: seq }), profileId, sql }
    set({ tabs: [...tabs, tab], active: tab.id, seq: seq + 1 })
    return tab.id
  },
  openData: (profileId, target) => {
    const { tabs } = get()
    const same = tabs.find((x) => sameData(x, profileId, target))
    if (same) {
      set({ active: same.id })
      return same.id
    }
    const data = { schema: target.schema, object: target.object, objectKind: target.objectKind }
    const tab: QueryTab = { id: newId(), title: target.object, profileId, sql: '', data }
    set({ tabs: [...tabs, tab], active: tab.id })
    return tab.id
  },
  openDesign: (profileId, target) => {
    const { tabs } = get()
    const same =
      target.table !== null &&
      tabs.find((x) => x.profileId === profileId && x.design?.schema === target.schema && x.design.table === target.table)
    if (same) {
      set({ active: same.id })
      return same.id
    }
    const title = target.table ?? t('design.newTable')
    const tab: QueryTab = { id: newId(), title, profileId, sql: '', design: { ...target } }
    set({ tabs: [...tabs, tab], active: tab.id })
    return tab.id
  },
  setDesign: (id, target) =>
    set({
      tabs: get().tabs.map((x) => (x.id === id ? { ...x, design: { ...target }, title: target.table ?? x.title } : x)),
    }),
  setData: (id, patch) =>
    set({ tabs: get().tabs.map((x) => (x.id === id && x.data ? { ...x, data: { ...x.data, ...patch } } : x)) }),
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

/** 打开后要立即执行的标签（运维查询，M1c3）。不随标签保存：重新打开页面时不会再执行。 */
const pendingRuns = new Set<string>()

export function markRun(tabId: string) {
  pendingRuns.add(tabId)
}

/** 取走"打开后执行"的标记：有标记时返回 true，只返回一次。 */
export function takeRun(tabId: string): boolean {
  return pendingRuns.delete(tabId)
}

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
