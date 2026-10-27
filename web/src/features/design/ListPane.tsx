// 索引、外键、唯一约束、检查约束共用的列表：工具栏（新增、删除、还原）和行内编辑的表格，带改动标记。

import { IconArrowBackUp, IconPlus, IconTrash } from '@tabler/icons-react'
import { Button, Table } from 'antd'
import type { ColumnsType } from 'antd/es/table'
import { useState } from 'react'
import { useTranslation } from 'react-i18next'

import { type Draft, type ListName, removeItem, restoreItem, rowState, sameContent } from './logic'

type Item<K extends ListName> = Draft[K][number]

interface Props<K extends ListName> {
  list: K
  draft: Draft
  original: Draft | null
  addLabel: string
  emptyText: string
  /** 新的一行。 */
  create(): Item<K>
  columns: ColumnsType<Item<K>>
  onChange(fn: (d: Draft) => Draft): void
  /** 工具栏右侧的说明。 */
  note?: React.ReactNode
  height?: string
}

export default function ListPane<K extends ListName>(p: Props<K>) {
  const { t } = useTranslation()
  const [selected, setSelected] = useState<string | null>(null)
  const rows = p.draft[p.list] as Item<K>[]
  const originals = p.original?.[p.list] as Item<K>[] | undefined
  const sel = rows.find((r) => r.key === selected)
  const state = (r: Item<K>) => rowState(r, originals, sameContent)
  const selState = sel ? state(sel) : null
  return (
    <div className="design-list">
      <div className="row toolbar">
        <Button
          size="small"
          type="text"
          icon={<IconPlus size={16} />}
          onClick={() => {
            const item = p.create()
            p.onChange((d) => ({ ...d, [p.list]: [...d[p.list], item] }))
            setSelected(item.key)
          }}
        >
          {p.addLabel}
        </Button>
        <Button
          size="small"
          type="text"
          icon={<IconTrash size={16} />}
          disabled={!sel || sel.deleted}
          onClick={() => sel && p.onChange((d) => removeItem(d, p.list, sel.key))}
        >
          {t('design.remove')}
        </Button>
        <Button
          size="small"
          type="text"
          icon={<IconArrowBackUp size={16} />}
          disabled={!sel || !p.original || (selState !== 'changed' && selState !== 'deleted')}
          onClick={() => sel && p.original && p.onChange((d) => restoreItem(d, p.list, sel.key, p.original!))}
        >
          {t('design.restore')}
        </Button>
        {p.note}
      </div>
      <Table<Item<K>>
        size="small"
        rowKey="key"
        className="design-table"
        pagination={false}
        dataSource={rows}
        columns={p.columns}
        locale={{ emptyText: p.emptyText }}
        scroll={p.height ? { y: p.height } : undefined}
        rowClassName={(r) => {
          const s = state(r)
          return `${s ? `design-${s}` : ''}${r.key === selected ? ' design-selected' : ''}`
        }}
        onRow={(r) => ({ onClick: () => setSelected(r.key) })}
      />
    </div>
  )
}
