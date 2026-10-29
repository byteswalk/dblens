// 图（graph.ts）转成 React Flow 的节点和连线：ER 图标签和导出文档时在屏幕外画图共用。

import { type Edge, MarkerType } from '@xyflow/react'

import type { ErPosition } from '../../shared/api'
import type { Graph } from './graph'
import TableNode, { type TableNodeType } from './TableNode'

export const NODE_TYPES = { table: TableNode }

export function flowNodes(graph: Graph, positions: Record<string, ErPosition>): TableNodeType[] {
  return graph.nodes.map((n) => ({
    id: n.id,
    type: 'table' as const,
    position: positions[n.id] ?? { x: 0, y: 0 },
    data: n.data,
    style: { width: n.width },
  }))
}

/** `columns(n)` 给出复合外键连线上的"N 列"。 */
export function flowEdges(graph: Graph, columns: (count: number) => string): Edge[] {
  return graph.edges.map((e) => ({
    id: e.id,
    source: e.source,
    target: e.target,
    sourceHandle: e.sourceHandle,
    targetHandle: e.targetHandle,
    type: 'smoothstep',
    label: e.count > 1 ? columns(e.count) : undefined,
    className: e.logical ? 'er-edge logical' : 'er-edge',
    markerEnd: { type: MarkerType.ArrowClosed },
    data: { title: e.title, logicalIndex: e.logicalIndex },
  }))
}
