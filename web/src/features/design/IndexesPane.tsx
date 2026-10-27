// "索引"子标签：名称、列（选择列或输入表达式，列名后加 DESC 表示降序）、唯一、方法。主键和唯一约束自带的索引不在这里。

import { Checkbox, Input, Select } from 'antd'
import { useTranslation } from 'react-i18next'

import type { DesignCaps } from '../../shared/api'
import { Cell } from './ColumnsPane'
import ListPane from './ListPane'
import { type Draft, type DraftIndex, type Issues, newKey, updateItem } from './logic'

const DESC = /\s+desc$/i

/** 索引列与标签文字互转：降序的列写成"列名 DESC"。 */
export const indexTags = (x: DraftIndex) => x.columns.map((c) => (c.descending ? `${c.name} DESC` : c.name))
export const fromTags = (tags: string[]) =>
  tags.map((s) => s.trim()).filter(Boolean).map((s) => ({ name: s.replace(DESC, '').trim(), descending: DESC.test(s) }))

export default function IndexesPane({ draft, original, caps, issues, onChange }: {
  draft: Draft
  original: Draft | null
  caps: DesignCaps | undefined
  issues: Issues
  onChange(fn: (d: Draft) => Draft): void
}) {
  const { t } = useTranslation()
  const names = draft.columns.filter((c) => !c.deleted && c.name).map((c) => ({ value: c.name }))
  const set = (key: string, patch: Partial<DraftIndex>) => onChange((d) => updateItem(d, 'indexes', key, patch))
  return (
    <ListPane
      list="indexes"
      draft={draft}
      original={original}
      onChange={onChange}
      addLabel={t('design.index.add')}
      emptyText={t('design.index.empty')}
      height="calc(100vh - 300px)"
      create={() => ({
        key: newKey(),
        origin: null,
        name: `ix_${draft.name || 't'}_${draft.indexes.length + 1}`,
        columns: [],
        unique: false,
        method: caps?.index_methods[0] ?? null,
      })}
      columns={[
        {
          title: t('design.index.name'),
          width: 220,
          render: (_, x) => (
            <Cell error={issues.at(x.key, 'name')}>
              <Input size="small" variant="borderless" value={x.name} disabled={x.deleted} onChange={(e) => set(x.key, { name: e.target.value })} />
            </Cell>
          ),
        },
        {
          title: t('design.index.columns'),
          render: (_, x) => (
            <Cell error={issues.at(x.key, 'columns')}>
              <Select
                mode="tags"
                size="small"
                variant="borderless"
                style={{ width: '100%' }}
                placeholder={t('design.index.columnsHint')}
                value={indexTags(x)}
                options={names}
                disabled={x.deleted}
                onChange={(tags: string[]) => set(x.key, { columns: fromTags(tags) })}
              />
            </Cell>
          ),
        },
        {
          title: t('design.index.unique'),
          width: 64,
          align: 'center',
          render: (_, x) => <Checkbox checked={x.unique} disabled={x.deleted} onChange={(e) => set(x.key, { unique: e.target.checked })} />,
        },
        {
          title: t('design.index.method'),
          width: 140,
          render: (_, x) => (
            <Select
              size="small"
              variant="borderless"
              style={{ width: '100%' }}
              value={x.method ?? undefined}
              allowClear
              disabled={x.deleted}
              options={(caps?.index_methods ?? []).map((m) => ({ value: m }))}
              onChange={(m?: string) => set(x.key, { method: m ?? null })}
            />
          ),
        },
      ]}
    />
  )
}
