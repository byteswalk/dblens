// 活动栏的"模型"面板（M4a 设计文档第 7 节）：打开的模型 → schema → 表（文件有错的标红），最近打开的模型；
// 工具栏打开、新建、从库反向生成、重新加载；右键新建表、用设计器打开、改名、删除、与库比对。
// 模型目录是服务所在机器上的路径：服务不允许时（监听其他地址的浏览器模式）只显示说明。

import { IconDatabaseImport, IconFolderOpen, IconPlus, IconRefresh, IconSchema, IconTable } from '@tabler/icons-react'
import { useQuery, useQueryClient } from '@tanstack/react-query'
import { App, Dropdown, Input, type MenuProps, Tree } from 'antd'
import type { DataNode } from 'antd/es/tree'
import { useState } from 'react'
import { useTranslation } from 'react-i18next'

import { useApi } from '../../app/context'
import { ApiError, type ModelSummary, type ModelTableEntry } from '../../shared/api'
import { desktop } from '../../shared/shell'
import { useTabs } from '../editor/tabs'
import { useModelDialogs } from './dialogs'

type NodeRef =
  | { type: 'model'; model: ModelSummary }
  | { type: 'schema'; model: ModelSummary; schema: string }
  | { type: 'table'; model: ModelSummary; entry: ModelTableEntry }

function ModelTree({ model, onMenu }: { model: ModelSummary; onMenu(n: NodeRef, key: string): void }) {
  const api = useApi()
  const { t } = useTranslation()
  const { data: tables = [] } = useQuery({ queryKey: ['model-tables', model.id], queryFn: () => api.modelTables(model.id) })
  const schemas = [...new Set([...model.info.schemas, ...tables.map((x) => x.schema)])]
  const refs = new Map<string, NodeRef>()
  const node = (key: string, ref: NodeRef, d: Omit<DataNode, 'key'>): DataNode => {
    refs.set(key, ref)
    return { key, ...d }
  }
  const tree: DataNode[] = [
    node(model.id, { type: 'model', model }, {
      title: <span title={model.path}>{model.info.name}{model.read_only && <span className="err"> · {t('model.readOnly')}</span>}</span>,
      icon: <IconSchema size={16} />,
      children: schemas.map((s) =>
        node(`${model.id}/${s}`, { type: 'schema', model, schema: s }, {
          title: s,
          children: tables
            .filter((x) => x.schema === s)
            .map((e) =>
              node(`${model.id}/${s}/${e.name}`, { type: 'table', model, entry: e }, {
                title: e.error ? (
                  <span className="err" title={`${e.error.path}: ${e.error.message}`}>{e.name}</span>
                ) : (
                  <span title={e.comment ?? undefined}>{e.name}</span>
                ),
                icon: <IconTable size={14} />,
                isLeaf: true,
              }),
            ),
        }),
      ),
    }),
  ]
  const [menuNode, setMenuNode] = useState<NodeRef | null>(null)
  const items = (n: NodeRef | null): MenuProps['items'] => {
    if (!n) return []
    const ro = n.model.read_only
    if (n.type === 'table') {
      return [
        { key: 'design', label: t('model.menu.design'), disabled: !!n.entry.error },
        { key: 'compare', label: t('model.menu.compare') },
        { type: 'divider' },
        { key: 'rename', label: t('model.menu.rename'), disabled: ro || !!n.entry.error },
        { key: 'delete', label: t('model.menu.delete'), danger: true, disabled: ro },
      ]
    }
    const common: NonNullable<MenuProps['items']> = [
      { key: 'new-table', label: t('model.menu.newTable'), disabled: ro },
      { key: 'compare', label: t('model.menu.compare') },
    ]
    if (n.type === 'schema') return common
    return [
      ...common,
      { type: 'divider' },
      { key: 'reload', label: t('model.menu.reload') },
      ...(desktop ? [{ key: 'reveal', label: t('model.menu.reveal') }] : []),
      { key: 'close', label: t('model.menu.close') },
    ]
  }
  return (
    <Dropdown trigger={['contextMenu']} menu={{ items: items(menuNode), onClick: ({ key }) => menuNode && onMenu(menuNode, key) }}>
      <div>
        <Tree
          showIcon
          blockNode
          defaultExpandedKeys={[model.id]}
          treeData={tree}
          onRightClick={({ node: n }) => setMenuNode(refs.get(String(n.key)) ?? null)}
          onDoubleClick={(_, n) => {
            const r = refs.get(String(n.key))
            if (r?.type === 'table' && !r.entry.error) onMenu(r, 'design')
          }}
        />
      </div>
    </Dropdown>
  )
}

export default function ModelsPanel() {
  const api = useApi()
  const { t } = useTranslation()
  const { message, modal } = App.useApp()
  const qc = useQueryClient()
  const dialogs = useModelDialogs()
  const { data, error, refetch } = useQuery({ queryKey: ['models'], queryFn: () => api.models(), retry: false })

  if (error) {
    const forbidden = error instanceof ApiError && error.status === 403
    return <div className="muted" style={{ padding: 12 }}>{forbidden ? t('model.localOnly') : (error as Error).message}</div>
  }

  const reload = async (m?: ModelSummary) => {
    if (m) await api.model(m.id).catch((e) => message.error((e as Error).message))
    await qc.invalidateQueries({ queryKey: ['models'] })
    await qc.invalidateQueries({ queryKey: m ? ['model-tables', m.id] : ['model-tables'] })
  }

  const onMenu = async (n: NodeRef, key: string) => {
    const m = n.model
    const schema = n.type === 'schema' ? n.schema : n.type === 'table' ? n.entry.schema : (m.info.schemas[0] ?? '')
    try {
      if (key === 'design' && n.type === 'table') useTabs.getState().openDesign('', { schema, table: n.entry.name, model: m.path })
      else if (key === 'new-table') useTabs.getState().openDesign('', { schema, table: null, model: m.path })
      else if (key === 'compare') useTabs.getState().openCompare({ source: { kind: 'model', path: m.path, schema }, target: null })
      else if (key === 'reload') await reload(m)
      else if (key === 'reveal') await desktop?.revealInFolder(`${m.path}/model.yaml`)
      else if (key === 'close') {
        await api.closeModel(m.id)
        await qc.invalidateQueries({ queryKey: ['models'] })
        qc.removeQueries({ queryKey: ['model-open', m.path] })
      } else if (key === 'rename' && n.type === 'table') {
        let to = n.entry.name
        modal.confirm({
          title: t('model.rename.title', { name: n.entry.name }),
          content: <Input defaultValue={to} onChange={(e) => (to = e.target.value)} />,
          onOk: async () => {
            if (!to.trim() || to === n.entry.name) return
            await api.renameModelTable(m.id, schema, n.entry.name, to.trim())
            await reload()
          },
        })
      } else if (key === 'delete' && n.type === 'table') {
        modal.confirm({
          title: t('model.delete.title', { name: n.entry.name }),
          content: t('model.delete.content'),
          okType: 'danger',
          onOk: async () => {
            await api.deleteModelTable(m.id, schema, n.entry.name)
            await reload()
          },
        })
      }
    } catch (e) {
      message.error((e as Error).message)
    }
  }

  const open = data?.open ?? []
  const recent = (data?.recent ?? []).filter((p) => !open.some((m) => m.path === p))
  return (
    <div className="models-panel">
      <div className="row" style={{ gap: 4, padding: '4px 8px' }}>
        <span className="a" title={t('model.toolbar.open')} onClick={dialogs.openOpen}><IconFolderOpen size={16} /></span>
        <span className="a" title={t('model.toolbar.create')} onClick={dialogs.openCreate}><IconPlus size={16} /></span>
        <span className="a" title={t('model.toolbar.reverse')} onClick={() => dialogs.openReverse()}><IconDatabaseImport size={16} /></span>
        <span className="grow" />
        <span className="a" title={t('model.toolbar.reload')} onClick={() => void reload().then(() => refetch())}><IconRefresh size={16} /></span>
      </div>
      {open.length === 0 && <div className="muted" style={{ padding: '4px 12px' }}>{t('model.empty')}</div>}
      {open.map((m) => (
        <ModelTree key={m.id} model={m} onMenu={(n, k) => void onMenu(n, k)} />
      ))}
      {recent.length > 0 && (
        <>
          <div className="muted" style={{ padding: '8px 12px 4px' }}>{t('model.recent')}</div>
          {recent.map((p) => (
            <div
              key={p}
              className="models-recent"
              title={p}
              onClick={() =>
                void api
                  .openModel(p)
                  .then(() => reload())
                  .catch((e) => message.error((e as Error).message))
              }
            >
              {p}
            </div>
          ))}
        </>
      )}
    </div>
  )
}
