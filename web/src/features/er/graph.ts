// ER 图的纯逻辑（M4b 设计文档 4.2、5.3 节）：表和外键换算成节点、连线；按显示方式算节点尺寸；dagre 自动布局。
// 不引入 React Flow，单独测试。

import { graphlib, layout as dagreLayout } from '@dagrejs/dagre'

import type { DesignTableModel, ErDisplay, ErPosition, ErTable } from '../../shared/api'

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
}

export interface ErNodeData extends Record<string, unknown> {
  name: string
  comment: string | null
  /** 文件有错（只显示名字）。 */
  error: string | null
  columns: ErColumn[]
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

interface KeyLike {
  name?: string | null
  columns: string[]
  ref_schema?: string | null
  ref_table: string
  ref_columns: string[]
}

const out = (col: string) => `${col}:out`
const into = (col: string) => `${col}:in`

/** "只显示键列"时显示的列：主键、外键（含逻辑外键）两端的列。 */
function keyColumns(t: ErTable, all: ErTable[], schema: string): Set<string> {
  const s = new Set<string>(t.table.primary_key?.columns ?? [])
  const keys = (x: ErTable): KeyLike[] => [...x.table.foreign_keys, ...x.logical_keys]
  for (const k of keys(t)) k.columns.forEach((c) => s.add(c))
  for (const other of all) {
    for (const k of keys(other)) {
      if ((k.ref_schema ?? schema) === schema && k.ref_table === t.table.name) k.ref_columns.forEach((c) => s.add(c))
    }
  }
  return s
}

export function nodeHeight(columns: number, external: number): number {
  return HEADER_HEIGHT + columns * ROW_HEIGHT + external * EXTERNAL_HEIGHT + 6
}

/** 表和外键换算成节点和连线。`schema` 是图所在的 schema（外键不写 schema 时指它）。 */
export function buildGraph(tables: ErTable[], display: ErDisplay, schema: string): Graph {
  const names = new Set(tables.map((t) => t.table.name))
  const nodes: ErNode[] = []
  const edges: ErEdge[] = []
  for (const t of tables) {
    const tab = t.table
    const fkCols = new Set([...tab.foreign_keys, ...t.logical_keys].flatMap((k) => k.columns))
    const pk = new Set(tab.primary_key?.columns ?? [])
    const shown = display === 'names' ? new Set<string>() : display === 'keys' ? keyColumns(t, tables, schema) : null
    const columns = t.error
      ? []
      : tab.columns
          .filter((c) => !shown || shown.has(c.name))
          .map((c) => ({ name: c.name, type: c.data_type.native, pk: pk.has(c.name), fk: fkCols.has(c.name), nullable: c.nullable }))
    const external: string[] = []
    const visible = new Set(columns.map((c) => c.name))
    const handle = (col: string | undefined, dir: (c: string) => string, cols: Set<string>) =>
      col !== undefined && cols.has(col) ? dir(col) : dir(TABLE_HANDLE)
    const add = (k: KeyLike, logical: boolean, index: number) => {
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
        id: `${logical ? 'lk' : 'fk'}:${tab.name}:${index}`,
        source: tab.name,
        target: k.ref_table,
        sourceHandle: handle(k.columns[0], out, visible),
        targetHandle: handle(k.ref_columns[0], into, targetVisible),
        logical,
        count: k.columns.length,
        title: k.name ? `${k.name}: ${pairs}` : pairs,
        logicalIndex: logical ? index : null,
      })
    }
    tab.foreign_keys.forEach((k, i) => add(k, false, i))
    t.logical_keys.forEach((k, i) => add(k, true, i))
    nodes.push({
      id: tab.name,
      data: { name: tab.name, comment: tab.comment, error: t.error ? t.error.message : null, columns, external },
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

/** 图中的表名（搜索定位用）：包含关键字的，不区分大小写。 */
export function searchTables(tables: DesignTableModel[], q: string): string[] {
  const k = q.trim().toLowerCase()
  if (!k) return []
  return tables.map((t) => t.name).filter((n) => n.toLowerCase().includes(k))
}
