// 少交叉布局：用 ELK 的 layered 算法排表、算直角连线。连线按列的端口计算，
// 打开业务主题时每个主题是一个容器，表留在自己的主题内。

import type { ElkExtendedEdge, ElkNode, LayoutOptions } from 'elkjs/lib/elk-api'

import type { ErPosition } from '../../shared/api'
import { type Graph, HEADER_HEIGHT, NODE_WIDTH, ROW_HEIGHT } from './graph'

export type ElkDirection = 'RIGHT' | 'DOWN'

export interface ElkPoint {
  x: number
  y: number
}

export interface ElkResult {
  positions: Record<string, ErPosition>
  /** 连线的 id → 折线上的点（画布坐标）。 */
  routes: Record<string, ElkPoint[]>
}

const OPTIONS: LayoutOptions = {
  'elk.algorithm': 'layered',
  'elk.edgeRouting': 'ORTHOGONAL',
  'elk.layered.crossingMinimization.strategy': 'LAYER_SWEEP',
  'elk.layered.crossingMinimization.greedySwitch.type': 'TWO_SIDED',
  'elk.layered.nodePlacement.strategy': 'NETWORK_SIMPLEX',
  'elk.layered.spacing.nodeNodeBetweenLayers': '90',
  'elk.spacing.nodeNode': '36',
  'elk.spacing.edgeEdge': '12',
  'elk.spacing.edgeNode': '20',
  'elk.padding': '[top=30,left=16,bottom=16,right=16]',
  // 坐标都相对画布，不相对所在的主题
  'elk.json.shapeCoords': 'ROOT',
  'elk.json.edgeCoords': 'ROOT',
}

/** 列（或表头）在节点上的纵向位置。 */
function portY(graph: Graph, id: string, handle: string): number {
  const column = handle.replace(/:(in|out)$/, '')
  const n = graph.nodes.find((x) => x.id === id)!
  const row = n.data.columns.findIndex((c) => c.name === column)
  return row < 0 ? HEADER_HEIGHT / 2 : HEADER_HEIGHT + row * ROW_HEIGHT + ROW_HEIGHT / 2
}

/** 图转成 ELK 的输入。自引用不参与布局。 */
export function toElk(graph: Graph, direction: ElkDirection, groups?: Map<string, string>): ElkNode {
  const edges = graph.edges.filter((e) => e.source !== e.target)
  const leaf = (id: string): ElkNode => {
    const n = graph.nodes.find((x) => x.id === id)!
    const ports = edges.flatMap((e) => {
      const out = e.source === id ? [{ id: `${e.id}:s`, x: NODE_WIDTH, y: portY(graph, id, e.sourceHandle), width: 0, height: 0 }] : []
      const into = e.target === id ? [{ id: `${e.id}:t`, x: 0, y: portY(graph, id, e.targetHandle), width: 0, height: 0 }] : []
      return [...out, ...into]
    })
    return {
      id,
      width: n.width,
      height: n.height,
      // 纵向时连线走上下，端口交给 ELK 自由放置
      ports: direction === 'RIGHT' ? ports : [],
      layoutOptions: direction === 'RIGHT' ? { 'elk.portConstraints': 'FIXED_POS' } : {},
    }
  }
  const links: ElkExtendedEdge[] = edges.map((e) => ({
    id: e.id,
    sources: [direction === 'RIGHT' ? `${e.id}:s` : e.source],
    targets: [direction === 'RIGHT' ? `${e.id}:t` : e.target],
  }))
  const options = { ...OPTIONS, 'elk.direction': direction }
  if (!groups) return { id: 'root', layoutOptions: options, children: graph.nodes.map((n) => leaf(n.id)), edges: links }
  const buckets = new Map<string, string[]>()
  for (const n of graph.nodes) {
    const key = groups.get(n.id) ?? ''
    buckets.set(key, [...(buckets.get(key) ?? []), n.id])
  }
  return {
    id: 'root',
    layoutOptions: { ...options, 'elk.hierarchyHandling': 'INCLUDE_CHILDREN' },
    children: [...buckets.entries()].map(([key, ids], i) => ({
      id: `topic:${i}:${key}`,
      layoutOptions: options,
      children: ids.map(leaf),
    })),
    edges: links,
  }
}

/** ELK 的结果转成位置和折线。 */
export function fromElk(root: ElkNode): ElkResult {
  const positions: Record<string, ErPosition> = {}
  const routes: Record<string, ElkPoint[]> = {}
  const walk = (node: ElkNode) => {
    for (const c of node.children ?? []) {
      if (c.children?.length) walk(c)
      else positions[c.id] = { x: Math.round(c.x ?? 0), y: Math.round(c.y ?? 0) }
    }
    for (const e of node.edges ?? []) {
      const s = e.sections?.[0]
      if (s) routes[e.id] = [s.startPoint, ...(s.bendPoints ?? []), s.endPoint]
    }
  }
  walk(root)
  return { positions, routes }
}

/** 折线转成 SVG 路径，拐角带小圆角。 */
export function routePath(points: ElkPoint[], radius = 6): string {
  let d = `M ${points[0].x} ${points[0].y}`
  for (let i = 1; i < points.length - 1; i++) {
    const [a, b, c] = [points[i - 1], points[i], points[i + 1]]
    const r = Math.min(radius, Math.hypot(b.x - a.x, b.y - a.y) / 2, Math.hypot(c.x - b.x, c.y - b.y) / 2)
    const toward = (p: ElkPoint) => {
      const len = Math.hypot(p.x - b.x, p.y - b.y) || 1
      return { x: b.x + ((p.x - b.x) / len) * r, y: b.y + ((p.y - b.y) / len) * r }
    }
    const [p, q] = [toward(a), toward(c)]
    d += ` L ${p.x} ${p.y} Q ${b.x} ${b.y} ${q.x} ${q.y}`
  }
  const end = points[points.length - 1]
  return `${d} L ${end.x} ${end.y}`
}

/** 在 Web Worker 里计算，大图不卡界面。库按需加载。 */
export async function layoutElk(graph: Graph, direction: ElkDirection, groups?: Map<string, string>): Promise<ElkResult> {
  const [{ default: ELK }, { default: workerUrl }] = await Promise.all([
    import('elkjs/lib/elk-api'),
    import('elkjs/lib/elk-worker.min.js?url'),
  ])
  const elk = new ELK({ workerUrl })
  try {
    return fromElk(await elk.layout(toElk(graph, direction, groups)))
  } finally {
    elk.terminateWorker()
  }
}
