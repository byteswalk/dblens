// ER 图标签（M4b 设计文档第 4 节）：模型的一张图（可编辑布局、逻辑外键、主题图的表，Ctrl+S 保存），
// 或连接的一个 schema（只读、自动布局、不保存）。画布用 React Flow，布局用 dagre，导出用 html-to-image。
// 引入 React Flow，由工作区懒加载。

import {
  IconAlertTriangle,
  IconDeviceFloppy,
  IconFocusCentered,
  IconLayoutDistributeHorizontal,
  IconPhoto,
  IconPlus,
  IconRefresh,
  IconSchema,
} from '@tabler/icons-react'
import { useQuery, useQueryClient } from '@tanstack/react-query'
import {
  Background,
  type Connection,
  Controls,
  type Edge,
  MarkerType,
  MiniMap,
  ReactFlow,
  ReactFlowProvider,
  getNodesBounds,
  useNodesState,
  useReactFlow,
} from '@xyflow/react'
import '@xyflow/react/dist/style.css'
import { Alert, App, AutoComplete, Badge, Button, Dropdown, Empty, Form, Input, Modal, Popover, Segmented, Select, Spin } from 'antd'
import { type KeyboardEvent, useCallback, useEffect, useMemo, useState } from 'react'
import { useTranslation } from 'react-i18next'

import { useApi } from '../../app/context'
import { type DesignTableModel, type ErDiagram, type ErDisplay, type ErPosition, type ErTable, DEFAULT_DIAGRAM } from '../../shared/api'
import { useSetting } from '../../shared/settings'
import { desktop } from '../../shared/shell'
import { THEME_KEY, parseThemePref, resolveTheme, useSystemDark } from '../../shared/theme'
import { withVault } from '../connections/vault'
import { type QueryTab, useTabs } from '../editor/tabs'
import { useModelDialogs } from '../model/dialogs'
import { type ErTabState, useErTabs } from './events'
import { type ImageFormat, dataUrlBase64, imageName, renderImage } from './export'
import { type Direction, TABLE_HANDLE, buildGraph, layout, searchTables } from './graph'
import TableNode, { type TableNodeType } from './TableNode'

const NODE_TYPES = { table: TableNode }
/** 默认图超过这么多张表时先只显示表名。 */
const LARGE = 300

interface Loaded {
  /** 可画的表：模型为 schema 的全部表（按图中的表过滤后画出），库为整个 schema。 */
  tables: ErTable[]
  diagram: ErDiagram
  fingerprint: string | null
  warnings: string[]
}

/** 拖线建逻辑外键时要填的名称、注释。 */
interface PendingKey {
  table: string
  column: string
  refTable: string
  refColumn: string
}

function Canvas({ tab }: { tab: QueryTab }) {
  const api = useApi()
  const { t } = useTranslation()
  const { message, modal } = App.useApp()
  const qc = useQueryClient()
  const flow = useReactFlow()
  const theme = resolveTheme(parseThemePref(useSetting(THEME_KEY)), useSystemDark())
  const target = tab.er!
  const isModel = target.kind === 'model'
  const schema = target.schema

  const modelQ = useQuery({
    queryKey: ['model-open', isModel ? target.path : null],
    queryFn: () => api.openModel((target as { path: string }).path),
    enabled: isModel,
    staleTime: Infinity,
  })
  const modelId = modelQ.data?.id
  const diagramName = isModel ? target.diagram : DEFAULT_DIAGRAM
  // 键以 model-tables 开头：设计器保存、模型面板中改表后一起刷新
  const dataQ = useQuery({
    queryKey: isModel ? ['model-tables', modelId, 'er', schema, diagramName] : ['er-db', target.profile, schema],
    queryFn: async (): Promise<Loaded> => {
      if (isModel) {
        const v = await api.diagram(modelId!, schema, diagramName)
        const warnings = v.warnings.map((w) =>
          w.kind === 'missing_table'
            ? t('er.warning.missingTable', { name: w.name })
            : t('er.warning.brokenKey', { table: w.table, name: w.name ?? `#${w.index + 1}` }),
        )
        for (const x of v.tables) if (x.error) warnings.push(t('er.warning.badFile', { name: x.table.name, message: x.error.message }))
        // 主题图：表从全部表中取（"添加表…"可选、加进来的表不必先保存就能画出来），按图中的表过滤
        const all = diagramName === DEFAULT_DIAGRAM ? v : await api.diagram(modelId!, schema, DEFAULT_DIAGRAM)
        return { tables: all.tables, diagram: v.diagram, fingerprint: v.fingerprint, warnings }
      }
      const tables = await withVault(api, () => api.schemaTables(target.profile, schema))
      return {
        tables: tables.map((table) => ({ table, logical_keys: [] })),
        diagram: { name: DEFAULT_DIAGRAM, tables: 'all', display: tables.length > LARGE ? 'names' : 'columns' },
        fingerprint: null,
        warnings: [],
      }
    },
    enabled: !isModel || !!modelId,
  })
  const data = dataQ.data

  // 本地的编辑：位置、显示方式、主题图的表；dirty 时标题标"未保存"
  // 编辑状态放在 store 中：切换到设计器等标签再回来时保留未保存的改动
  const st = useErTabs((s) => s.tabs[tab.id])
  const positions = useMemo(() => st?.positions ?? {}, [st?.positions])
  const display = st?.display ?? 'columns'
  const tableList = st?.tables ?? 'all'
  const fingerprint = st?.fingerprint ?? null
  const dirty = st?.dirty ?? false
  const patch = useCallback((p: Partial<ErTabState>) => useErTabs.getState().patch(tab.id, p), [tab.id])
  const setPositions = (p: Record<string, ErPosition>) => patch({ positions: p })
  const [exporting, setExporting] = useState(false)
  const [pending, setPending] = useState<PendingKey | null>(null)
  const [adding, setAdding] = useState(false)

  // 第一次读到、文件变了（且没有未保存的改动）时取文件中的布局；之后的刷新保留未保存的改动
  useEffect(() => {
    if (!data) return
    const key = `${tab.id}:${data.fingerprint ?? 'none'}`
    const cur = useErTabs.getState().tabs[tab.id]
    if (!cur || (!cur.dirty && cur.loadedKey !== key)) {
      const large = data.fingerprint === null && data.tables.length > LARGE
      useErTabs.getState().set(tab.id, {
        positions: data.diagram.positions ?? {},
        display: large ? 'names' : data.diagram.display,
        tables: data.diagram.tables,
        fingerprint: data.fingerprint,
        dirty: false,
        loadedKey: key,
      })
    }
  }, [data, tab.id])

  const tables = useMemo(
    () => (data?.tables ?? []).filter((x) => tableList === 'all' || tableList.includes(x.table.name)),
    [data, tableList],
  )
  const graph = useMemo(() => buildGraph(tables, display, schema), [tables, display, schema])

  // 没有位置的表自动排
  useEffect(() => {
    if (!st || !graph.nodes.length || graph.nodes.every((n) => positions[n.id])) return
    patch({ positions: layout(graph, positions) })
  }, [st, graph, positions, patch])

  const [nodes, setNodes, onNodesChange] = useNodesState<TableNodeType>([])
  useEffect(() => {
    setNodes((prev) =>
      graph.nodes.map((n) => ({
        id: n.id,
        type: 'table' as const,
        position: positions[n.id] ?? { x: 0, y: 0 },
        data: n.data,
        selected: prev.find((p) => p.id === n.id)?.selected ?? false,
        style: { width: n.width },
      })),
    )
  }, [graph, positions, setNodes])

  const edges: Edge[] = useMemo(
    () =>
      graph.edges.map((e) => ({
        id: e.id,
        source: e.source,
        target: e.target,
        sourceHandle: e.sourceHandle,
        targetHandle: e.targetHandle,
        type: 'smoothstep',
        label: e.count > 1 ? t('er.columns', { count: e.count }) : undefined,
        className: e.logical ? 'er-edge logical' : 'er-edge',
        markerEnd: { type: MarkerType.ArrowClosed },
        data: { title: e.title, logicalIndex: e.logicalIndex },
      })),
    [graph, t],
  )

  const markDirty = () => isModel && patch({ dirty: true })

  const autoLayout = (direction: Direction, all: boolean) => {
    setPositions(layout(graph, positions, { direction, all }))
    markDirty()
    setTimeout(() => void flow.fitView({ padding: 0.1 }), 50)
  }

  const save = useCallback(async () => {
    if (!isModel || !modelId || !data || !dirty) return
    const diagram: ErDiagram = { ...data.diagram, display, tables: tableList, positions }
    try {
      const fp = await api.saveDiagram(modelId, schema, diagramName, diagram, fingerprint)
      patch({ fingerprint: fp, dirty: false, loadedKey: `${tab.id}:${fp}` })
      message.success(t('er.saved'))
      void qc.invalidateQueries({ queryKey: ['model-diagrams', modelId] })
      void dataQ.refetch()
    } catch (e) {
      message.error((e as Error).message)
    }
  }, [isModel, modelId, data, dirty, display, tableList, positions, api, schema, diagramName, fingerprint, message, t, qc, dataQ, patch, tab.id])

  const reload = () => {
    const go = async () => {
      useErTabs.getState().clear(tab.id)
      // 库：先让服务端的元数据缓存失效，否则读到的还是缓存中的表
      if (target.kind === 'database') await api.refresh(target.profile, schema).catch(() => {})
      void dataQ.refetch()
    }
    if (dirty) modal.confirm({ title: t('er.reloadConfirm'), okType: 'danger', onOk: go })
    else void go()
  }


  const onConnect = (c: Connection) => {
    if (!isModel || !c.sourceHandle || !c.targetHandle) return
    const col = (h: string) => h.replace(/:(in|out)$/, '')
    const column = col(c.sourceHandle)
    const refColumn = col(c.targetHandle)
    if (column === TABLE_HANDLE || refColumn === TABLE_HANDLE) {
      message.info(t('er.key.needColumns'))
      return
    }
    setPending({ table: c.source, column, refTable: c.target, refColumn })
  }

  const deleteKey = (e: Edge) => {
    const index = (e.data as { logicalIndex: number | null } | undefined)?.logicalIndex
    if (!isModel || index === null || index === undefined) return
    modal.confirm({
      title: t('er.key.deleteTitle'),
      content: (e.data as { title: string }).title,
      okType: 'danger',
      onOk: async () => {
        await api.deleteLogicalKey(modelId!, schema, e.source, index)
        void dataQ.refetch()
      },
    })
  }

  const openTable = (name: string) => {
    if (target.kind === 'model') useTabs.getState().openDesign('', { schema, table: name, model: target.path })
    else useTabs.getState().openData(target.profile, { schema, object: name, objectKind: 'table' })
  }

  const newTable = () => {
    if (target.kind !== 'model') return
    const id = useTabs.getState().openDesign('', { schema, table: null, model: target.path })
    useErTabs.getState().wait(id, tab.id)
  }

  const exportImage = async (format: ImageFormat, white: boolean) => {
    setExporting(true)
    // 去掉选中的标记；关掉"只渲染可见区域"，等全部节点画出来
    setNodes((ns) => ns.map((x) => ({ ...x, selected: false })))
    await new Promise((r) => setTimeout(r, 150))
    try {
      const viewport = document.querySelector<HTMLElement>(`[data-er-tab="${tab.id}"] .react-flow__viewport`)
      if (!viewport) return
      const bg = white ? '#ffffff' : getComputedStyle(document.body).getPropertyValue('--bg').trim() || '#ffffff'
      const url = await renderImage(viewport, getNodesBounds(flow.getNodes()), format, bg)
      const base = isModel ? (diagramName === DEFAULT_DIAGRAM ? schema : diagramName) : schema
      const name = imageName(base, format)
      if (desktop) {
        const path = await desktop.saveDialog(name, [{ name: format.toUpperCase(), extensions: [format] }])
        if (path) {
          await api.saveImage(path, dataUrlBase64(url))
          message.success(t('er.exported'))
        }
      } else {
        const a = document.createElement('a')
        a.href = url
        a.download = name
        a.click()
      }
    } catch (e) {
      message.error((e as Error).message)
    } finally {
      setExporting(false)
    }
  }

  const [search, setSearch] = useState('')
  const locate = (name: string) => {
    const n = flow.getNode(name)
    if (!n) return
    setNodes((ns) => ns.map((x) => ({ ...x, selected: x.id === name })))
    void flow.setCenter(n.position.x + (n.measured?.width ?? 240) / 2, n.position.y + 40, { zoom: 1, duration: 300 })
  }

  const [hover, setHover] = useState<{ x: number; y: number; text: string } | null>(null)
  const [menu, setMenu] = useState<{ x: number; y: number; items: { key: string; label: string; danger?: boolean }[]; on(key: string): void } | null>(null)

  const onKeyDown = (e: KeyboardEvent) => {
    if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === 's') {
      e.preventDefault()
      void save()
    }
  }

  if (modelQ.error) return <Alert type="error" style={{ margin: 12 }} message={(modelQ.error as Error).message} />
  if (dataQ.error) return <Alert type="error" style={{ margin: 12 }} message={(dataQ.error as Error).message} action={<Button onClick={reload}>{t('er.reload')}</Button>} />
  if (!data) return <Spin style={{ margin: 48 }} />

  const readOnly = !isModel || !!modelQ.data?.read_only
  const isTopic = tableList !== 'all'
  const title = isModel
    ? `${modelQ.data?.info.name ?? ''} / ${schema} · ${diagramName === DEFAULT_DIAGRAM ? t('er.defaultDiagram') : diagramName}`
    : `${schema} · ${t('er.readOnly')}`
  return (
    <div className="er-view" data-er-tab={tab.id} onKeyDown={onKeyDown} tabIndex={-1}>
      <div className="row toolbar" style={{ gap: 6, flexWrap: 'wrap' }}>
        <b>{title}</b>
        {dirty && <span className="err">● {t('er.unsaved')}</span>}
        <span className="grow" />
        <AutoComplete
          size="small"
          style={{ width: 160 }}
          placeholder={t('er.search')}
          value={search}
          onChange={setSearch}
          options={searchTables(tables.map((x) => x.table), search).map((n) => ({ value: n }))}
          onSelect={(v: string) => locate(v)}
        />
        <Segmented
          size="small"
          value={display}
          onChange={(v) => {
            patch({ display: v as ErDisplay })
            markDirty()
          }}
          options={[
            { value: 'columns', label: t('er.display.columns') },
            { value: 'keys', label: t('er.display.keys') },
            { value: 'names', label: t('er.display.names') },
          ]}
        />
        <Dropdown
          menu={{
            items: [
              { key: 'LR:all', label: t('er.layout.horizontal') },
              { key: 'TB:all', label: t('er.layout.vertical') },
            ],
            onClick: ({ key }) => {
              const [d, mode] = key.split(':')
              autoLayout(d as Direction, mode === 'all')
            },
          }}
        >
          <Button size="small" type="text" icon={<IconLayoutDistributeHorizontal size={16} />}>
            {t('er.layout.title')}
          </Button>
        </Dropdown>
        <Button size="small" type="text" icon={<IconFocusCentered size={16} />} title={t('er.fit')} onClick={() => void flow.fitView({ padding: 0.1 })} />
        {data.warnings.length > 0 && (
          <Popover
            content={
              <ul className="er-warnings">
                {data.warnings.map((w) => (
                  <li key={w}>{w}</li>
                ))}
              </ul>
            }
          >
            <Badge count={data.warnings.length} size="small">
              <Button size="small" type="text" icon={<IconAlertTriangle size={16} />} />
            </Badge>
          </Popover>
        )}
        {isModel && isTopic && !readOnly && (
          <Button size="small" type="text" icon={<IconPlus size={16} />} onClick={() => setAdding(true)}>
            {t('er.addTables')}
          </Button>
        )}
        <Dropdown
          menu={{
            items: [
              { key: 'svg', label: t('er.export.svg') },
              { key: 'png', label: t('er.export.png') },
              { key: 'svg:white', label: t('er.export.svgWhite') },
              { key: 'png:white', label: t('er.export.pngWhite') },
            ],
            onClick: ({ key }) => {
              const [f, w] = key.split(':')
              void exportImage(f as ImageFormat, w === 'white')
            },
          }}
        >
          <Button size="small" type="text" loading={exporting} icon={<IconPhoto size={16} />}>
            {t('er.export.title')}
          </Button>
        </Dropdown>
        <Button size="small" type="text" icon={<IconRefresh size={16} />} title={t('er.reload')} onClick={reload} />
        {isModel ? (
          <Button size="small" type="primary" icon={<IconDeviceFloppy size={16} />} disabled={!dirty || readOnly} onClick={() => void save()}>
            {t('er.save')}
          </Button>
        ) : (
          <Button
            size="small"
            icon={<IconSchema size={16} />}
            onClick={() => useModelDialogs.getState().openReverse({ profileId: target.profile, schema })}
          >
            {t('er.saveAsModel')}
          </Button>
        )}
      </div>
      <div className="er-canvas grow">
        {data && !data.tables.length && <Empty className="er-empty" description={t(tableList === 'all' ? 'er.empty' : 'er.emptyDiagram')} />}
        <ReactFlow
          nodes={nodes}
          edges={edges}
          nodeTypes={NODE_TYPES}
          onNodesChange={onNodesChange}
          onNodeDragStop={(_, __, dragged) => {
            const next = { ...positions }
            for (const n of dragged) next[n.id] = { x: Math.round(n.position.x), y: Math.round(n.position.y) }
            setPositions(next)
            markDirty()
          }}
          onNodeDoubleClick={(_, n) => !n.data.error && openTable(n.id)}
          onConnect={onConnect}
          nodesConnectable={!readOnly}
          edgesFocusable
          onEdgesDelete={(es) => es.forEach(deleteKey)}
          deleteKeyCode={readOnly ? null : 'Delete'}
          onBeforeDelete={async ({ nodes: ns, edges: es }) => ns.length === 0 && es.every((e) => e.id.startsWith('lk:'))}
          onNodeContextMenu={(e, n) => {
            e.preventDefault()
            const items = isModel
              ? [
                  { key: 'design', label: t('er.menu.design') },
                  ...(isTopic && !readOnly ? [{ key: 'remove', label: t('er.menu.remove') }] : []),
                ]
              : [
                  { key: 'data', label: t('er.menu.data') },
                  { key: 'design-db', label: t('er.menu.design') },
                ]
            setMenu({
              x: e.clientX,
              y: e.clientY,
              items,
              on: (key) => {
                if (key === 'design' || key === 'data') openTable(n.id)
                else if (key === 'design-db' && target.kind === 'database') useTabs.getState().openDesign(target.profile, { schema, table: n.id })
                else if (key === 'remove' && isTopic) {
                  patch({ tables: (tableList as string[]).filter((x) => x !== n.id), dirty: true })
                }
              },
            })
          }}
          onEdgeContextMenu={(e, edge) => {
            e.preventDefault()
            if (!edge.id.startsWith('lk:') || readOnly) return
            setMenu({ x: e.clientX, y: e.clientY, items: [{ key: 'delete', label: t('er.key.delete'), danger: true }], on: () => deleteKey(edge) })
          }}
          onPaneContextMenu={(e) => {
            e.preventDefault()
            if (!isModel || readOnly) return
            setMenu({ x: e.clientX, y: e.clientY, items: [{ key: 'new', label: t('er.menu.newTable') }], on: newTable })
          }}
          onPaneClick={() => setMenu(null)}
          onEdgeMouseEnter={(e, edge) => setHover({ x: e.clientX, y: e.clientY, text: (edge.data as { title: string }).title })}
          onEdgeMouseLeave={() => setHover(null)}
          onlyRenderVisibleElements={!exporting}
          colorMode={theme}
          fitView
          minZoom={0.05}
          maxZoom={2}
          proOptions={{ hideAttribution: true }}
        >
          <Background gap={16} />
          <Controls showInteractive={false} />
          <MiniMap pannable zoomable />
        </ReactFlow>
        {hover && (
          <div className="er-tip" style={{ left: hover.x + 12, top: hover.y + 12 }}>
            {hover.text}
          </div>
        )}
        {menu && (
          <Dropdown
            open
            onOpenChange={(o) => !o && setMenu(null)}
            menu={{
              items: menu.items,
              onClick: ({ key }) => {
                menu.on(key)
                setMenu(null)
              },
            }}
          >
            <span style={{ position: 'fixed', left: menu.x, top: menu.y }} />
          </Dropdown>
        )}
      </div>
      {pending && (
        <KeyDialog
          pending={pending}
          onCancel={() => setPending(null)}
          onOk={async (name, comment) => {
            try {
              await api.addLogicalKey(modelId!, schema, pending.table, {
                name: name || null,
                columns: [pending.column],
                ref_table: pending.refTable,
                ref_columns: [pending.refColumn],
                comment: comment || null,
              })
              setPending(null)
              void dataQ.refetch()
            } catch (e) {
              message.error((e as Error).message)
            }
          }}
        />
      )}
      {adding && data && (
        <AddTablesDialog
          all={data.tables.map((x) => x.table)}
          current={tableList === 'all' ? [] : tableList}
          onCancel={() => setAdding(false)}
          onOk={(names) => {
            patch({ tables: [...(tableList === 'all' ? [] : tableList), ...names], dirty: true })
            setAdding(false)
          }}
        />
      )}
    </div>
  )
}

function KeyDialog({ pending, onOk, onCancel }: { pending: PendingKey; onOk(name: string, comment: string): void; onCancel(): void }) {
  const { t } = useTranslation()
  const [name, setName] = useState(`lk_${pending.table}_${pending.refTable}`)
  const [comment, setComment] = useState('')
  return (
    <Modal open title={t('er.key.title')} onCancel={onCancel} onOk={() => onOk(name.trim(), comment.trim())}>
      <p>
        {pending.table}.{pending.column} → {pending.refTable}.{pending.refColumn}
      </p>
      <Form layout="vertical">
        <Form.Item label={t('er.key.name')}>
          <Input value={name} onChange={(e) => setName(e.target.value)} />
        </Form.Item>
        <Form.Item label={t('er.key.comment')}>
          <Input value={comment} onChange={(e) => setComment(e.target.value)} />
        </Form.Item>
      </Form>
    </Modal>
  )
}

function AddTablesDialog({ all, current, onOk, onCancel }: {
  all: DesignTableModel[]
  current: string[]
  onOk(names: string[]): void
  onCancel(): void
}) {
  const { t } = useTranslation()
  const [picked, setPicked] = useState<string[]>([])
  const options = all.filter((x) => !current.includes(x.name)).map((x) => ({ value: x.name }))
  return (
    <Modal open title={t('er.addTables')} onCancel={onCancel} onOk={() => onOk(picked)} okButtonProps={{ disabled: !picked.length }}>
      <Select mode="multiple" style={{ width: '100%' }} value={picked} onChange={setPicked} options={options} showSearch />
    </Modal>
  )
}

export default function ErView({ tab }: { tab: QueryTab }) {
  return (
    <ReactFlowProvider>
      <Canvas tab={tab} />
    </ReactFlowProvider>
  )
}
