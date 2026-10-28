// 设计文档的 ER 图图片（M4c 设计文档 3.3 节）：在屏幕外挂一个不可交互的 React Flow，复用 ER 图的建图、布局和表节点，
// 浅色主题、白底，逐张画成 PNG 并上传。引入 React Flow，由对话框懒加载。

import { ReactFlow, ReactFlowProvider } from '@xyflow/react'
import '@xyflow/react/dist/style.css'
import { createRoot } from 'react-dom/client'

import type { Api, DocDiagramImage, ErDisplay, ErPosition, ErTable } from '../../shared/api'
import { renderImage } from '../er/export'
import { NODE_TYPES, flowEdges, flowNodes } from '../er/flow'
import { buildGraph, layout } from '../er/graph'
import { pixelRatio } from './options'

/** 一张要画的图。 */
export interface DiagramJob {
  schema: string
  name: string
  title: string
  description: string | null
  tables: ErTable[]
  display: ErDisplay
  positions: Record<string, ErPosition>
  /** 表较多、已改为只显示表名（库图按 ER 图的规则）：文档中注明。 */
  namesOnly: boolean
}

const PADDING = 24
const frame = () => new Promise((r) => requestAnimationFrame(() => r(null)))

/** 画一张图：返回 PNG 的 data URL，以及是否改成了只显示表名。 */
async function draw(job: DiagramJob, columns: (n: number) => string): Promise<{ url: string; namesOnly: boolean }> {
  const host = document.createElement('div')
  host.className = 'er-canvas'
  host.dataset.theme = 'light'
  host.style.cssText = 'position:fixed;left:-30000px;top:0;width:1600px;height:1200px;pointer-events:none'
  document.body.appendChild(host)
  const root = createRoot(host)
  try {
    for (const display of job.display === 'names' ? (['names'] as const) : [job.display, 'names' as const]) {
      const graph = buildGraph(job.tables, display, job.schema)
      const positions = graph.nodes.every((n) => job.positions[n.id]) ? job.positions : layout(graph, job.positions)
      root.render(
        <ReactFlowProvider>
          <ReactFlow
            nodes={flowNodes(graph, positions)}
            edges={flowEdges(graph, columns)}
            nodeTypes={NODE_TYPES}
            colorMode="light"
            nodesDraggable={false}
            nodesConnectable={false}
            proOptions={{ hideAttribution: true }}
          />
        </ReactFlowProvider>,
      )
      // 等节点画出来、连线算好
      for (let i = 0; i < 3; i++) await frame()
      await new Promise((r) => setTimeout(r, 100))
      const viewport = host.querySelector<HTMLElement>('.react-flow__viewport')
      if (!viewport) throw new Error('viewport')
      const xs = graph.nodes.map((n) => positions[n.id] ?? { x: 0, y: 0 })
      const minX = Math.min(0, ...xs.map((p) => p.x))
      const minY = Math.min(0, ...xs.map((p) => p.y))
      const maxX = Math.max(1, ...graph.nodes.map((n, i) => xs[i].x + n.width))
      const maxY = Math.max(1, ...graph.nodes.map((n, i) => xs[i].y + n.height))
      const bounds = { x: minX, y: minY, width: maxX - minX, height: maxY - minY }
      const ratio = pixelRatio(bounds.width + PADDING * 2, bounds.height + PADDING * 2)
      if (ratio === 0 && display !== 'names') continue
      const url = await renderImage(viewport, bounds, 'png', '#ffffff', ratio || 1)
      return { url, namesOnly: display === 'names' && job.display !== 'names' }
    }
    throw new Error('unreachable')
  } finally {
    root.unmount()
    host.remove()
  }
}

/** 逐张画图并上传；`onProgress(已完成, 总数)`；`cancelled()` 为真时停下。 */
export async function renderDiagrams(
  api: Api,
  jobs: DiagramJob[],
  columns: (n: number) => string,
  onProgress: (done: number, total: number) => void,
  cancelled: () => boolean,
): Promise<DocDiagramImage[] | null> {
  const out: DocDiagramImage[] = []
  for (const [i, job] of jobs.entries()) {
    if (cancelled()) return null
    onProgress(i, jobs.length)
    const { url, namesOnly } = await draw(job, columns)
    const blob = await (await fetch(url)).blob()
    const file = new File([blob], `${job.schema}-${job.name}.png`, { type: 'image/png' })
    const up = await api.upload(file)
    out.push({
      upload: up.id,
      schema: job.schema,
      name: job.name,
      title: job.title,
      description: job.description,
      names_only: namesOnly || job.namesOnly,
    })
  }
  onProgress(jobs.length, jobs.length)
  return out
}
