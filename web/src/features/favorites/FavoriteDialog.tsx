// 收藏对话框（M1c1 设计文档第 6 节）：收藏 SQL（名称默认取 SQL 开头、选文件夹、说明）、新建文件夹、编辑。
// 编辑时也可以换文件夹（移到末尾），不用拖动也能整理。

import { App, Form, Input, Modal, TreeSelect } from 'antd'
import { useQuery, useQueryClient } from '@tanstack/react-query'
import { useEffect, useMemo } from 'react'
import { useTranslation } from 'react-i18next'

import { useApi } from '../../app/context'
import type { Favorite } from '../../shared/api'
import { useSettings } from '../../shared/settings'
import { LAST_FOLDER_KEY, useFavoriteDialog } from './dialog'
import { type FavNode, buildTree, canMoveInto, defaultName } from './tree'

interface Values {
  name: string
  folder: string
  sql?: string
  note?: string
}

/** 文件夹选择框中根节点的值（TreeSelect 把 null 和空字符串当作没有选择）。 */
const ROOT = '#root'

interface FolderOption {
  value: string
  title: string
  disabled: boolean
  children: FolderOption[]
}

/** 文件夹树；`allowed` 为 false 的不能选（编辑文件夹时不能选它自己和它的下级）。 */
function folderOptions(nodes: FavNode[], allowed: (id: string) => boolean): FolderOption[] {
  return nodes
    .filter((n) => n.fav.kind === 'folder')
    .map((n) => ({
      value: n.fav.id,
      title: n.fav.name,
      disabled: !allowed(n.fav.id),
      children: folderOptions(n.children, allowed),
    }))
}

export default function FavoriteDialog() {
  const api = useApi()
  const qc = useQueryClient()
  const { t } = useTranslation()
  const { message } = App.useApp()
  const { draft, close } = useFavoriteDialog()
  const [form] = Form.useForm<Values>()
  const { data: favs = [] } = useQuery({ queryKey: ['favorites'], queryFn: () => api.favorites(), enabled: !!draft })
  const editing = draft?.mode === 'edit' ? draft.fav : null
  const folders = useMemo(() => {
    const allowed = (id: string) => !editing || canMoveInto(favs, editing.id, id)
    return [{ value: ROOT, title: t('app.favorites.root'), disabled: false, children: folderOptions(buildTree(favs), allowed) }]
  }, [favs, t, editing])

  const isItem = draft !== null && (draft.mode === 'edit' ? draft.fav.kind === 'item' : draft.kind === 'item')

  useEffect(() => {
    if (!draft) return
    const last = useSettings.getState().values[LAST_FOLDER_KEY]
    const lastFolder = last && favs.some((f) => f.id === last && f.kind === 'folder') ? last : ROOT
    if (draft.mode === 'edit') {
      form.setFieldsValue({ name: draft.fav.name, folder: draft.fav.parent_id ?? ROOT, sql: draft.fav.sql ?? undefined, note: draft.fav.note ?? '' })
    } else if (draft.kind === 'item') {
      form.setFieldsValue({ name: defaultName(draft.sql), folder: lastFolder, sql: draft.sql, note: '' })
    } else {
      form.setFieldsValue({ name: '', folder: draft.parentId ?? ROOT })
    }
    // 只在打开时填一次（不依赖 favs）：文件夹列表晚到时不覆盖用户已改的内容
  }, [draft, form])

  const submit = async () => {
    if (!draft) return
    const v = await form.validateFields()
    try {
      let saved: Favorite
      if (draft.mode === 'edit') {
        const parent = v.folder === ROOT ? null : v.folder
        saved = await api.updateFavorite(draft.fav.id, {
          name: v.name,
          ...(isItem ? { sql: v.sql, note: v.note ?? '' } : {}),
          // 换了文件夹：放在新文件夹的末尾（服务端把超出的位置当作末尾）
          ...(parent !== draft.fav.parent_id ? { move: { parent_id: parent, position: Number.MAX_SAFE_INTEGER } } : {}),
        })
      } else {
        const parent = v.folder === ROOT ? null : v.folder
        saved = await api.createFavorite(
          draft.kind === 'item'
            ? { parent_id: parent, kind: 'item', name: v.name, sql: v.sql, note: v.note || undefined, driver: draft.driver, variant: draft.variant }
            : { parent_id: parent, kind: 'folder', name: v.name },
        )
        if (draft.kind === 'item') useSettings.getState().set(LAST_FOLDER_KEY, parent)
      }
      await qc.invalidateQueries({ queryKey: ['favorites'] })
      if (draft.mode === 'create' && draft.kind === 'item') message.success(t('app.favorites.saved', { name: saved.name }))
      close()
    } catch (e) {
      message.error((e as Error).message)
    }
  }

  const title = !draft
    ? ''
    : draft.mode === 'edit'
      ? t(isItem ? 'app.favorites.editTitle' : 'app.favorites.renameTitle')
      : t(isItem ? 'app.favorites.addTitle' : 'app.favorites.newFolderTitle')

  return (
    <Modal title={title} open={!!draft} onOk={() => void submit()} onCancel={close} destroyOnHidden width={isItem ? 560 : 400}>
      <Form form={form} layout="vertical" style={{ marginTop: 12 }}>
        <Form.Item name="name" label={t('app.favorites.name')} rules={[{ required: true, whitespace: true, message: t('app.favorites.nameRequired') }]}>
          <Input autoFocus maxLength={200} />
        </Form.Item>
        <Form.Item name="folder" label={t('app.favorites.folder')}>
          <TreeSelect treeData={folders} treeDefaultExpandAll />
        </Form.Item>
        {isItem && (
          <>
            <Form.Item name="sql" label="SQL" rules={[{ required: true, whitespace: true, message: t('app.favorites.sqlRequired') }]}>
              <Input.TextArea autoSize={{ minRows: 3, maxRows: 12 }} className="mono" />
            </Form.Item>
            <Form.Item name="note" label={t('app.favorites.note')}>
              <Input.TextArea autoSize={{ minRows: 1, maxRows: 4 }} />
            </Form.Item>
          </>
        )}
      </Form>
    </Modal>
  )
}
