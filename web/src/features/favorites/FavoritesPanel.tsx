// 收藏侧栏（M1c1 设计文档第 6 节）：多级文件夹，可拖动整理；双击插入到当前编辑器；
// 右键在新标签中打开、编辑（含换文件夹）、新建子文件夹、删除（文件夹删除前确认）。

import { App, Button, Dropdown, Tooltip, Tree } from 'antd'
import type { DataNode, TreeProps } from 'antd/es/tree'
import { IconFileCode, IconFolder, IconFolderPlus } from '@tabler/icons-react'
import { useQuery, useQueryClient } from '@tanstack/react-query'
import { useMemo } from 'react'
import { useTranslation } from 'react-i18next'

import { useApi } from '../../app/context'
import DbIcon from '../../shared/DbIcon'
import type { Favorite } from '../../shared/api'
import { useQueryActions } from '../editor/actions'
import { useFavoriteDialog } from './dialog'
import { type FavNode, buildTree, dropTarget } from './tree'

export default function FavoritesPanel() {
  const api = useApi()
  const qc = useQueryClient()
  const { t } = useTranslation()
  const { message, modal } = App.useApp()
  const { insertSql, openSql } = useQueryActions()
  const open = useFavoriteDialog((s) => s.open)
  const query = useQuery({ queryKey: ['favorites'], queryFn: () => api.favorites() })
  const favs = useMemo(() => query.data ?? [], [query.data])

  const guard = async (p: Promise<unknown>) => {
    try {
      await p
    } catch (e) {
      message.error((e as Error).message)
    }
    await qc.invalidateQueries({ queryKey: ['favorites'] })
  }

  const remove = (f: Favorite) => {
    if (f.kind === 'item') return void guard(api.deleteFavorite(f.id))
    modal.confirm({
      title: t('app.favorites.deleteFolderConfirm', { name: f.name }),
      okButtonProps: { danger: true },
      onOk: () => guard(api.deleteFavorite(f.id)),
    })
  }

  const menu = (f: Favorite) => ({
    items:
      f.kind === 'item'
        ? [
            { key: 'open', label: t('app.favorites.openInTab') },
            { key: 'edit', label: t('app.favorites.edit') },
            { type: 'divider' as const },
            { key: 'delete', label: t('app.favorites.delete'), danger: true },
          ]
        : [
            { key: 'new-folder', label: t('app.favorites.newSubfolder') },
            { key: 'edit', label: t('app.favorites.edit') },
            { type: 'divider' as const },
            { key: 'delete', label: t('app.favorites.delete'), danger: true },
          ],
    onClick: ({ key }: { key: string }) => {
      if (key === 'open' && f.sql) openSql(f.sql)
      else if (key === 'edit') open({ mode: 'edit', fav: f })
      else if (key === 'new-folder') open({ mode: 'create', kind: 'folder', parentId: f.id })
      else if (key === 'delete') remove(f)
    },
  })

  const treeData = useMemo(() => {
    const toData = (nodes: FavNode[]): DataNode[] =>
      nodes.map(({ fav, children }) => ({
        key: fav.id,
        isLeaf: fav.kind === 'item',
        icon:
          fav.kind === 'folder' ? (
            <IconFolder size={14} />
          ) : fav.driver ? (
            <DbIcon driver={fav.driver} variant={fav.variant} />
          ) : (
            <IconFileCode size={14} />
          ),
        title: fav.name,
        children: fav.kind === 'folder' ? toData(children) : undefined,
      }))
    return toData(buildTree(favs))
  }, [favs])

  const onDrop: TreeProps['onDrop'] = (info) => {
    const pos = Number(info.node.pos.split('-').pop())
    const where = !info.dropToGap ? 0 : info.dropPosition - pos < 0 ? -1 : 1
    const move = dropTarget(favs, String(info.dragNode.key), String(info.node.key), where)
    if (move) void guard(api.updateFavorite(String(info.dragNode.key), { move }))
  }

  const byId = useMemo(() => new Map(favs.map((f) => [f.id, f])), [favs])

  return (
    <div className="fav">
      <div className="row fav-bar">
        <span className="grow muted">{t('app.favorites.hint')}</span>
        <Tooltip title={t('app.favorites.newFolder')}>
          <Button type="text" size="small" icon={<IconFolderPlus size={16} />} onClick={() => open({ mode: 'create', kind: 'folder', parentId: null })} />
        </Tooltip>
      </div>
      {query.error ? (
        <div className="hist-empty">
          <div className="err">{(query.error as Error).message}</div>
          <Button size="small" onClick={() => void query.refetch()}>
            {t('app.history.retry')}
          </Button>
        </div>
      ) : !query.isPending && favs.length === 0 ? (
        <div className="hist-empty muted">{t('app.favorites.empty')}</div>
      ) : (
        <Tree
          className="fav-tree"
          showIcon
          blockNode
          draggable={{ icon: false }}
          defaultExpandAll
          treeData={treeData}
          onDrop={onDrop}
          titleRender={(node) => {
            const f = byId.get(String(node.key))
            if (!f) return null
            const label = (
              <span
                className="fav-title"
                onDoubleClick={() => {
                  if (f.sql) insertSql(f.sql)
                }}
              >
                {f.name}
              </span>
            )
            return (
              <Dropdown trigger={['contextMenu']} menu={menu(f)}>
                {f.kind === 'item' ? (
                  <Tooltip
                    placement="right"
                    mouseEnterDelay={0.6}
                    styles={{ root: { maxWidth: 560 } }}
                    title={
                      <>
                        {f.note && <div style={{ marginBottom: 4 }}>{f.note}</div>}
                        <pre className="hist-preview">{f.sql}</pre>
                      </>
                    }
                  >
                    {label}
                  </Tooltip>
                ) : (
                  label
                )}
              </Dropdown>
            )
          }}
        />
      )}
    </div>
  )
}
