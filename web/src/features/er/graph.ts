// ER 图的纯逻辑（M4b 设计文档 4.2、5.3 节）：表和外键换算成节点、连线；按显示方式算节点尺寸；dagre 自动布局。
// 不引入 React Flow，单独测试。

import { graphlib, layout as dagreLayout } from '@dagrejs/dagre'

import { type DesignTableModel, type ErDisplay, type ErPosition, type ErTable, splitCatalog } from '../../shared/api'

export const NODE_WIDTH = 240
export const HEADER_HEIGHT = 30
export const ROW_HEIGHT = 22
export const EXTERNAL_HEIGHT = 20
/** 显示方式为"只显示表名"时连线接在表头上。 */
export const TABLE_HANDLE = '__table'

export interface ErColumn {
  name: string
  type: string
  pk: boolean
  fk: boolean
  nullable: boolean
  comment: string | null
}

export interface ErNodeData extends Record<string, unknown> {
  name: string
  comment: string | null
  /** 文件有错（只显示名字）。 */
  error: string | null
  columns: ErColumn[]
  /** 在图上显示注释（不显示时悬停也能看到）。 */
  showComments: boolean
  /** 引用图外的表（其他 schema、主题图中没有的表）："→ crm.customers"。 */
  external: string[]
}

export interface ErNode {
  id: string
  data: ErNodeData
  width: number
  height: number
}

export interface ErEdge {
  id: string
  source: string
  target: string
  sourceHandle: string
  targetHandle: string
  logical: boolean
  /** 按列名推测出来的，数据库和模型里都没有这个外键。 */
  inferred: boolean
  /** 复合外键的列数（大于 1 时显示）。 */
  count: number
  /** 悬停显示：约束名和列的对应。 */
  title: string
  /** 逻辑外键在源表 `logical_keys` 中的序号（删除用）。 */
  logicalIndex: number | null
}

export interface Graph {
  nodes: ErNode[]
  edges: ErEdge[]
}

/** 带推测外键的表。 */
type Inferred = ErTable & { inferred_keys?: KeyLike[] }

export interface KeyLike {
  name?: string | null
  columns: string[]
  ref_schema?: string | null
  ref_table: string
  ref_columns: string[]
}

const out = (col: string) => `${col}:out`
const into = (col: string) => `${col}:in`

/** "只显示键列"时显示的列：主键、外键（含逻辑外键）两端的列。 */
function keyColumns(t: Inferred, all: Inferred[], schema: string): Set<string> {
  const s = new Set<string>(t.table.primary_key?.columns ?? [])
  const keys = (x: Inferred): KeyLike[] => [...x.table.foreign_keys, ...x.logical_keys, ...(x.inferred_keys ?? [])]
  for (const k of keys(t)) k.columns.forEach((c) => s.add(c))
  for (const other of all) {
    for (const k of keys(other)) {
      if ((k.ref_schema ?? schema) === schema && k.ref_table === t.table.name) k.ref_columns.forEach((c) => s.add(c))
    }
  }
  return s
}

/** 列类型的大类：推测外键时两边要同类。 */
function typeClass(native: string): string {
  const t = native.toLowerCase()
  if (/int|serial|number|numeric|decimal/.test(t)) return 'number'
  if (/char|text|string/.test(t)) return 'string'
  return t.replace(/\(.*$/, '')
}

/** 表名去掉常见前缀，用来和列名比较。 */
const bareName = (name: string) => name.toLowerCase().replace(/^(t|tb|tbl|sys|biz)_/, '')

/**
 * 按列名推测外键：`dept_id`、`deptid` 指向表 dept（或 depts），或者列名和别的表的单列主键相同；
 * `parent_id` 指向本表。要求类型同类、目标唯一，有歧义时不推测。已有外键的列不再推测。
 */
export function inferKeys(tables: ErTable[]): Map<string, KeyLike[]> {
  const single = (t: ErTable) => (t.table.primary_key?.columns.length === 1 ? t.table.primary_key.columns[0] : null)
  const typeOf = (t: ErTable, col: string) => t.table.columns.find((c) => c.name === col)?.data_type.native ?? ''
  const result = new Map<string, KeyLike[]>()
  for (const t of tables) {
    if (t.error) continue
    const taken = new Set([...t.table.foreign_keys, ...t.logical_keys].flatMap((k) => k.columns))
    const found: KeyLike[] = []
    for (const c of t.table.columns) {
      const lower = c.name.toLowerCase()
      if (taken.has(c.name) || c.name === single(t)) continue
      const stem = lower.match(/^(.+?)_?id$/)?.[1]
      if (!stem) continue
      const forms = new Set([stem, `${stem}s`, `${stem}es`, stem.replace(/y$/, 'ies')])
      const type = typeClass(c.data_type.native)
      const targets = tables.filter((x) => {
        const pk = single(x)
        if (x.error || !pk || typeClass(typeOf(x, pk)) !== type) return false
        if (x === t) return lower === 'parent_id' || lower === 'parentid'
        return forms.has(bareName(x.table.name)) || pk.toLowerCase() === lower
      })
      if (targets.length === 1) {
        found.push({ columns: [c.name], ref_table: targets[0].table.name, ref_columns: [single(targets[0])!] })
      }
    }
    if (found.length) result.set(t.table.name, found)
  }
  return result
}

export function nodeHeight(columns: number, external: number): number {
  return HEADER_HEIGHT + columns * ROW_HEIGHT + external * EXTERNAL_HEIGHT + 6
}

/** 表和外键换算成节点和连线。`schema` 是图所在的 schema（外键不写 schema 时指它）；SQL Server 的"库 + 架构"
 * 只取架构名与外键比较。 */
export function buildGraph(
  input: ErTable[],
  display: ErDisplay,
  graphSchema: string,
  { inferred = false, comments = false }: { inferred?: boolean; comments?: boolean } = {},
): Graph {
  const schema = splitCatalog(graphSchema).schema
  const guessed = inferred ? inferKeys(input) : new Map<string, KeyLike[]>()
  const tables: Inferred[] = input.map((t) => ({ ...t, inferred_keys: guessed.get(t.table.name) ?? [] }))
  const names = new Set(tables.map((t) => t.table.name))
  const nodes: ErNode[] = []
  const edges: ErEdge[] = []
  for (const t of tables) {
    const tab = t.table
    const fkCols = new Set([...tab.foreign_keys, ...t.logical_keys, ...(t.inferred_keys ?? [])].flatMap((k) => k.columns))
    const pk = new Set(tab.primary_key?.columns ?? [])
    const shown = display === 'names' ? new Set<string>() : display === 'keys' ? keyColumns(t, tables, schema) : null
    const columns = t.error
      ? []
      : tab.columns
          .filter((c) => !shown || shown.has(c.name))
          .map((c) => ({ name: c.name, type: c.data_type.native, pk: pk.has(c.name), fk: fkCols.has(c.name), comment: c.comment ?? null, nullable: c.nullable }))
    const external: string[] = []
    const visible = new Set(columns.map((c) => c.name))
    const handle = (col: string | undefined, dir: (c: string) => string, cols: Set<string>) =>
      col !== undefined && cols.has(col) ? dir(col) : dir(TABLE_HANDLE)
    const add = (k: KeyLike, kind: 'fk' | 'lk' | 'ik', index: number) => {
      const refSchema = k.ref_schema ?? schema
      if (refSchema !== schema || !names.has(k.ref_table)) {
        const label = `→ ${refSchema !== schema ? `${refSchema}.` : ''}${k.ref_table}`
        if (!external.includes(label)) external.push(label)
        return
      }
      const target = tables.find((x) => x.table.name === k.ref_table)!
      const targetVisible = new Set(
        display === 'names'
          ? []
          : display === 'keys'
            ? [...keyColumns(target, tables, schema)]
            : target.table.columns.map((c) => c.name),
      )
      const pairs = k.columns.map((c, i) => `${c} → ${k.ref_columns[i] ?? '?'}`).join(', ')
      edges.push({
        id: `${kind}:${tab.name}:${index}`,
        source: tab.name,
        target: k.ref_table,
        sourceHandle: handle(k.columns[0], out, visible),
        targetHandle: handle(k.ref_columns[0], into, targetVisible),
        logical: kind !== 'fk',
        inferred: kind === 'ik',
        count: k.columns.length,
        title: k.name ? `${k.name}: ${pairs}` : pairs,
        logicalIndex: kind === 'lk' ? index : null,
      })
    }
    tab.foreign_keys.forEach((k, i) => add(k, 'fk', i))
    t.logical_keys.forEach((k, i) => add(k, 'lk', i))
    t.inferred_keys?.forEach((k, i) => add(k, 'ik', i))
    nodes.push({
      id: tab.name,
      data: { name: tab.name, comment: tab.comment, error: t.error ? t.error.message : null, columns, showComments: comments, external },
      width: NODE_WIDTH,
      height: nodeHeight(columns.length, external.length),
    })
  }
  return { nodes, edges }
}

export type Direction = 'LR' | 'TB'

/**
 * 自动布局。`all` 为 true 时全部重排；否则保留已记的位置，没有位置的表按 dagre 的顺序排在已有部分的下方
 * （全都没有位置时直接用 dagre 的结果）。返回每张表左上角的位置。
 */
export function layout(
  graph: Graph,
  positions: Record<string, ErPosition>,
  { direction = 'LR', all = false }: { direction?: Direction; all?: boolean } = {},
): Record<string, ErPosition> {
  const g = new graphlib.Graph()
  g.setGraph({ rankdir: direction, nodesep: 40, ranksep: 80, marginx: 20, marginy: 20 })
  g.setDefaultEdgeLabel(() => ({}))
  for (const n of graph.nodes) g.setNode(n.id, { width: n.width, height: n.height })
  for (const e of graph.edges) if (e.source !== e.target) g.setEdge(e.source, e.target)
  dagreLayout(g)
  const auto: Record<string, ErPosition> = {}
  for (const n of graph.nodes) {
    const p = g.node(n.id)
    auto[n.id] = { x: Math.round(p.x - n.width / 2), y: Math.round(p.y - n.height / 2) }
  }
  const kept = all ? {} : Object.fromEntries(Object.entries(positions).filter(([k]) => auto[k]))
  const missing = graph.nodes.filter((n) => !kept[n.id])
  if (!Object.keys(kept).length) return auto
  // 已有部分的下方，按 dagre 结果的左右顺序排成一行
  const bottom = Math.max(...graph.nodes.filter((n) => kept[n.id]).map((n) => kept[n.id].y + n.height))
  const left = Math.min(...Object.values(kept).map((p) => p.x))
  const result = { ...kept }
  let x = left
  for (const n of [...missing].sort((a, b) => auto[a.id].x - auto[b.id].x || auto[a.id].y - auto[b.id].y)) {
    result[n.id] = { x, y: bottom + 80 }
    x += n.width + 40
  }
  return result
}

export type PageOrientation = 'portrait' | 'landscape'

const PAGE_GAP = 30
/** A4 按 70% 缩放打印时一页放得下的画布尺寸，这个比例下 12px 的字还看得清。 */
const PAGE_LONG = 1600
const PAGE_SHORT = 1130
const PAGE_SPACING = 120

export interface ErPage {
  x: number
  y: number
  width: number
  height: number
}

/** 表按连线关系排顺序：相关的表靠近。 */
function linkedOrder(nodes: ErNode[], edges: ErEdge[]): ErNode[] {
  const links = new Map(nodes.map((n) => [n.id, new Set<string>()]))
  for (const e of edges) {
    if (e.source === e.target || !links.has(e.source) || !links.has(e.target)) continue
    links.get(e.source)!.add(e.target)
    links.get(e.target)!.add(e.source)
  }
  const byLinks = [...nodes].sort((a, b) => links.get(b.id)!.size - links.get(a.id)!.size)
  const order: ErNode[] = []
  const seen = new Set<string>()
  for (const start of byLinks) {
    const queue = [start]
    while (queue.length) {
      const n = queue.shift()!
      if (seen.has(n.id)) continue
      seen.add(n.id)
      order.push(n)
      queue.push(...nodes.filter((x) => links.get(n.id)!.has(x.id) && !seen.has(x.id)))
    }
  }
  return order
}

/**
 * 按多张 A4 排：保持能看清的大小，一页满了换下一页。给出 `groups` 时同一主题的表排在一起，
 * 每个主题从新的一列开始。返回位置和各页的范围（页面从左到右排开）。
 */
export function layoutPages(
  graph: Graph,
  orientation: PageOrientation,
  groups?: Map<string, string>,
): { positions: Record<string, ErPosition>; pages: ErPage[] } {
  const [width, height] = orientation === 'portrait' ? [PAGE_SHORT, PAGE_LONG] : [PAGE_LONG, PAGE_SHORT]
  const perPage = Math.max(1, Math.floor((width - 2 * PAGE_GAP + PAGE_GAP) / (NODE_WIDTH + PAGE_GAP)))
  const buckets = new Map<string, ErNode[]>()
  for (const n of graph.nodes) {
    const key = groups?.get(n.id) ?? ''
    buckets.set(key, [...(buckets.get(key) ?? []), n])
  }
  const positions: Record<string, ErPosition> = {}
  let [page, col, y] = [0, 0, PAGE_GAP]
  const nextCol = () => {
    y = PAGE_GAP
    col += 1
    if (col >= perPage) {
      col = 0
      page += 1
    }
  }
  for (const nodes of buckets.values()) {
    if (y > PAGE_GAP) nextCol()
    for (const n of linkedOrder(nodes, graph.edges)) {
      if (y > PAGE_GAP && y + n.height > height - PAGE_GAP) nextCol()
      positions[n.id] = { x: page * (width + PAGE_SPACING) + PAGE_GAP + col * (NODE_WIDTH + PAGE_GAP), y }
      y += n.height + PAGE_GAP
    }
  }
  const pages = Array.from({ length: page + 1 }, (_, i) => ({ x: i * (width + PAGE_SPACING), y: 0, width, height }))
  return { positions, pages }
}

/**
 * 按表名自动分业务主题：表名按下划线分段，大多数表共用的第一段是系统前缀（如 SZSS_），
 * 取下一段作主题；只有一张表的主题和分不出的表归入 `other`。返回 表名 → 主题。
 */
export function tableGroups(names: string[], other: string): Map<string, string> {
  const parts = names.map((n) => n.toUpperCase().split('_').filter(Boolean))
  const firsts = new Map<string, number>()
  for (const p of parts) firsts.set(p[0] ?? '', (firsts.get(p[0] ?? '') ?? 0) + 1)
  const common = Math.max(0, ...firsts.values())
  const index = names.length >= 4 && common / names.length >= 0.6 ? 1 : 0
  const keyOf = (p: string[]) => (p.length > index + 1 || (index === 0 && p.length > 1) ? p[index] : other)
  const counts = new Map<string, number>()
  for (const p of parts) counts.set(keyOf(p), (counts.get(keyOf(p)) ?? 0) + 1)
  return new Map(names.map((n, i) => {
    const key = keyOf(parts[i])
    return [n, (counts.get(key) ?? 0) >= 2 ? key : other]
  }))
}

/** 主题的边框：同一主题同一页的表围成一块。 */
export function groupFrames(
  graph: Graph,
  positions: Record<string, ErPosition>,
  groups: Map<string, string>,
  pages: ErPage[],
): (ErPage & { label: string })[] {
  const boxes = new Map<string, ErPage & { label: string }>()
  for (const n of graph.nodes) {
    const p = positions[n.id]
    if (!p) continue
    const label = groups.get(n.id) ?? ''
    const page = pages.findIndex((x) => p.x >= x.x && p.x < x.x + x.width)
    const key = `${label}|${page}`
    const box = boxes.get(key)
    const [right, bottom] = [p.x + n.width, p.y + n.height]
    if (!box) boxes.set(key, { x: p.x, y: p.y, width: right, height: bottom, label })
    else {
      box.x = Math.min(box.x, p.x)
      box.y = Math.min(box.y, p.y)
      box.width = Math.max(box.width, right)
      box.height = Math.max(box.height, bottom)
    }
  }
  const pad = 12
  return [...boxes.values()].map((b) => ({
    label: b.label,
    x: b.x - pad,
    y: b.y - pad - 18,
    width: b.width - b.x + 2 * pad,
    height: b.height - b.y + 2 * pad + 18,
  }))
}

/**
 * 铺满 A4 纸：表按连线关系排顺序（相关的表靠近），逐张放进当前最矮的一列。
 * 试各种列数，取缩放到纸上时比例最大的一种，所以表能印得尽量大。
 */
export function layoutPage(graph: Graph, orientation: PageOrientation): Record<string, ErPosition> {
  const nodes = graph.nodes
  if (!nodes.length) return {}
  const order = linkedOrder(nodes, graph.edges)
  const ratio = orientation === 'portrait' ? 210 / 297 : 297 / 210
  let best: { scale: number; positions: Record<string, ErPosition> } | null = null
  for (let cols = 1; cols <= order.length; cols++) {
    const heights = new Array<number>(cols).fill(0)
    const positions: Record<string, ErPosition> = {}
    for (const n of order) {
      const col = heights.indexOf(Math.min(...heights))
      positions[n.id] = { x: col * (NODE_WIDTH + PAGE_GAP), y: heights[col] }
      heights[col] += n.height + PAGE_GAP
    }
    const width = cols * NODE_WIDTH + (cols - 1) * PAGE_GAP
    const scale = Math.min(ratio / width, 1 / (Math.max(...heights) - PAGE_GAP))
    if (!best || scale > best.scale) best = { scale, positions }
  }
  return best!.positions
}

/** 图中的表名（搜索定位用）：包含关键字的，不区分大小写。 */
export function searchTables(tables: DesignTableModel[], q: string): string[] {
  const k = q.trim().toLowerCase()
  if (!k) return []
  return tables.map((t) => t.name).filter((n) => n.toLowerCase().includes(k))
}
