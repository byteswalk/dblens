// 图（graph.ts）转成 React Flow 的节点和连线：ER 图标签和导出文档时在屏幕外画图共用。

import { type Edge, MarkerType } from '@xyflow/react'

import type { ErPosition } from '../../shared/api'
import ErEdgeLine from './ErEdgeLine'
import type { ElkPoint } from './elk'
import { type ErEdge, type ErPage, type Graph, NODE_WIDTH } from './graph'
import TableNode, { FrameNode, type FrameNodeType, type TableNodeType } from './TableNode'

export const NODE_TYPES = { table: TableNode, frame: FrameNode }
export const EDGE_TYPES = { er: ErEdgeLine }

export function flowNodes(graph: Graph, positions: Record<string, ErPosition>): TableNodeType[] {
  return graph.nodes.map((n) => ({
    id: n.id,
    type: 'table' as const,
    position: positions[n.id] ?? { x: 0, y: 0 },
    data: n.data,
    style: { width: n.width },
    // 屏幕外的表不渲染、量不到尺寸，框选时会被当成在框内
    initialWidth: n.width,
    initialHeight: n.height,
  }))
}

/** A4 页面、业务主题的背景节点。 */
export function frameNode(id: string, box: ErPage, label: string, kind: 'page' | 'group'): FrameNodeType {
  return {
    id: `frame:${id}`,
    type: 'frame',
    position: { x: box.x, y: box.y },
    data: { label, kind },
    style: { width: box.width, height: box.height },
    selectable: false,
    draggable: false,
    focusable: false,
    zIndex: -1,
  }
}

/**
 * 连线从离目标近的一侧出去：目标在左边时走左侧，上下叠放时两端都在右侧，自引用绕左侧。
 * 同一张表同一侧的线分到不同通道。`columns(n)` 给出复合外键连线上的"N 列"。
 */
export function flowEdges(
  graph: Graph,
  columns: (count: number) => string,
  positions: Record<string, ErPosition> = {},
  inferredLabel = '',
  cardLabel?: (e: ErEdge) => string,
  routes: Record<string, ElkPoint[]> = {},
): Edge[] {
  const sides = graph.edges.map((e) => {
    const [a, b] = [positions[e.source], positions[e.target]]
    if (e.source === e.target) return { from: 'left', to: 'left' }
    if (!a || !b) return { from: 'right', to: 'left' }
    if (b.x >= a.x + NODE_WIDTH / 2) return { from: 'right', to: 'left' }
    if (b.x + NODE_WIDTH / 2 <= a.x) return { from: 'left', to: 'right' }
    return { from: 'right', to: 'right' }
  })
  const counts = new Map<string, number>()
  graph.edges.forEach((e, i) => {
    const key = `${e.source}:${sides[i].from}`
    counts.set(key, (counts.get(key) ?? 0) + 1)
  })
  const used = new Map<string, number>()
  return graph.edges.map((e, i) => {
    const { from, to } = sides[i]
    const key = `${e.source}:${from}`
    const n = used.get(key) ?? 0
    used.set(key, n + 1)
    const className = e.inferred ? 'er-edge logical inferred' : e.logical ? 'er-edge logical' : 'er-edge'
    return {
      id: e.id,
      source: e.source,
      target: e.target,
      sourceHandle: from === 'left' ? `${e.sourceHandle}:l` : e.sourceHandle,
      targetHandle: to === 'right' ? `${e.targetHandle}:r` : e.targetHandle,
      type: 'er',
      label: e.count > 1 ? columns(e.count) : undefined,
      className,
      markerEnd: { type: MarkerType.ArrowClosed },
      data: {
        title: `${cardLabel ? `${cardLabel(e)}  ` : ''}${e.inferred ? `${inferredLabel} ` : ''}${e.title}`,
        cards: cardLabel ? [e.sourceCard, e.targetCard] : undefined,
        route: routes[e.id],
        logicalIndex: e.logicalIndex,
        lane: n - ((counts.get(key) ?? 1) - 1) / 2,
      },
    }
  })
}
