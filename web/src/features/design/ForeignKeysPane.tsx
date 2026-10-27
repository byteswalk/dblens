// "外键"子标签：名称、列、引用的 schema 和表（从元数据选）、引用列、删除时和更新时的动作（库不支持的禁用）。

import { useQuery } from '@tanstack/react-query'
import { Input, Select } from 'antd'
import { useTranslation } from 'react-i18next'

import { useApi } from '../../app/context'
import type { DesignCaps } from '../../shared/api'
import { Cell } from './ColumnsPane'
import ListPane from './ListPane'
import { type Draft, type DraftForeignKey, type Issues, newKey, updateItem } from './logic'

/** 引用的表的列（选了表之后才查）。 */
function RefColumns({ profileId, schema, fk, onChange }: {
  profileId: string
  schema: string
  fk: DraftForeignKey
  onChange(cols: string[]): void
}) {
  const api = useApi()
  const refSchema = fk.ref_schema || schema
  const { data } = useQuery({
    queryKey: ['table-info', profileId, refSchema, fk.ref_table],
    queryFn: () => api.table(profileId, refSchema, fk.ref_table),
    enabled: !!fk.ref_table,
  })
  return (
    <Select
      mode="multiple"
      size="small"
      variant="borderless"
      style={{ width: '100%' }}
      value={fk.ref_columns}
      disabled={fk.deleted}
      options={(data?.columns ?? []).map((c) => ({ value: c.name }))}
      onChange={onChange}
    />
  )
}

export default function ForeignKeysPane({ profileId, schema, draft, original, caps, issues, onChange }: {
  profileId: string
  schema: string
  draft: Draft
  original: Draft | null
  caps: DesignCaps | undefined
  issues: Issues
  onChange(fn: (d: Draft) => Draft): void
}) {
  const { t } = useTranslation()
  const api = useApi()
  const { data: schemas = [] } = useQuery({ queryKey: ['schemas', profileId], queryFn: () => api.schemas(profileId) })
  const refSchemas = [...new Set(draft.foreignKeys.map((f) => f.ref_schema || schema))]
  const tables = useQuery({
    queryKey: ['design-ref-tables', profileId, refSchemas],
    queryFn: async () =>
      Object.fromEntries(await Promise.all(refSchemas.map(async (s) => [s, (await api.objects(profileId, s, 'tables')).map((o) => o.name)] as const))),
    enabled: !!schema,
  })
  const names = draft.columns.filter((c) => !c.deleted && c.name).map((c) => ({ value: c.name }))
  const set = (key: string, patch: Partial<DraftForeignKey>) => onChange((d) => updateItem(d, 'foreignKeys', key, patch))
  const action = (values: string[]) => values.map((v) => ({ value: v }))
  return (
    <ListPane
      list="foreignKeys"
      draft={draft}
      original={original}
      onChange={onChange}
      addLabel={t('design.fk.add')}
      emptyText={t('design.fk.empty')}
      height="calc(100vh - 300px)"
      create={() => ({
        key: newKey(),
        origin: null,
        name: `fk_${draft.name || 't'}_${draft.foreignKeys.length + 1}`,
        columns: [],
        ref_schema: null,
        ref_table: '',
        ref_columns: [],
        on_delete: null,
        on_update: null,
      })}
      columns={[
        {
          title: t('design.fk.name'),
          width: 160,
          render: (_, f) => (
            <Cell error={issues.at(f.key, 'name')}>
              <Input size="small" variant="borderless" value={f.name ?? ''} disabled={f.deleted} onChange={(e) => set(f.key, { name: e.target.value || null })} />
            </Cell>
          ),
        },
        {
          title: t('design.fk.columns'),
          width: 170,
          render: (_, f) => (
            <Cell error={issues.at(f.key, 'columns')}>
              <Select mode="multiple" size="small" variant="borderless" style={{ width: '100%' }} value={f.columns} options={names} disabled={f.deleted} onChange={(columns: string[]) => set(f.key, { columns })} />
            </Cell>
          ),
        },
        {
          title: t('design.fk.refSchema'),
          width: 130,
          render: (_, f) => (
            <Select
              size="small"
              variant="borderless"
              style={{ width: '100%' }}
              value={f.ref_schema || schema}
              disabled={f.deleted}
              showSearch
              options={schemas.map((s) => ({ value: s.name }))}
              onChange={(v: string) => set(f.key, { ref_schema: v === schema ? null : v, ref_table: '', ref_columns: [] })}
            />
          ),
        },
        {
          title: t('design.fk.refTable'),
          width: 160,
          render: (_, f) => (
            <Cell error={issues.at(f.key, 'ref_table')}>
              <Select
                size="small"
                variant="borderless"
                style={{ width: '100%' }}
                value={f.ref_table || undefined}
                disabled={f.deleted}
                showSearch
                options={(tables.data?.[f.ref_schema || schema] ?? []).map((n) => ({ value: n }))}
                onChange={(v: string) => set(f.key, { ref_table: v, ref_columns: [] })}
              />
            </Cell>
          ),
        },
        {
          title: t('design.fk.refColumns'),
          width: 170,
          render: (_, f) => (
            <Cell error={issues.at(f.key, 'ref_columns')}>
              <RefColumns profileId={profileId} schema={schema} fk={f} onChange={(ref_columns) => set(f.key, { ref_columns })} />
            </Cell>
          ),
        },
        {
          title: t('design.fk.onDelete'),
          width: 120,
          render: (_, f) => (
            <Select
              size="small"
              variant="borderless"
              style={{ width: '100%' }}
              allowClear
              value={f.on_delete ?? undefined}
              disabled={f.deleted || !caps?.on_delete.length}
              options={action(caps?.on_delete ?? [])}
              onChange={(v?: string) => set(f.key, { on_delete: v ?? null })}
            />
          ),
        },
        {
          title: t('design.fk.onUpdate'),
          width: 120,
          render: (_, f) => (
            <Select
              size="small"
              variant="borderless"
              style={{ width: '100%' }}
              allowClear
              placeholder={caps && !caps.on_update.length ? t('design.fk.unsupported') : undefined}
              value={f.on_update ?? undefined}
              disabled={f.deleted || !caps?.on_update.length}
              options={action(caps?.on_update ?? [])}
              onChange={(v?: string) => set(f.key, { on_update: v ?? null })}
            />
          ),
        },
      ]}
    />
  )
}
