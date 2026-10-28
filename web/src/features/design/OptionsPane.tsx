// "选项"子标签：schema（新建表时可选）、表名（只有新建时可改）、表注释、主键名和主键列的顺序、分区（只读）、行数估计和存储属性。

import { IconArrowDown, IconArrowUp } from '@tabler/icons-react'
import { useQuery } from '@tanstack/react-query'
import { Button, Input, Select } from 'antd'
import { useTranslation } from 'react-i18next'

import type { DesignTableResult } from '../../shared/api'
import { Cell } from './ColumnsPane'
import { type Draft, type Issues, moveItem } from './logic'
import type { RefSource } from './source'

export default function OptionsPane({ source, schema, isNew, draft, storage, issues, onSchema, onChange }: {
  source: RefSource
  schema: string
  isNew: boolean
  draft: Draft
  storage: DesignTableResult['storage'] | null
  issues: Issues
  onSchema(schema: string): void
  onChange(fn: (d: Draft) => Draft): void
}) {
  const { t } = useTranslation()
  const { data: schemas = [] } = useQuery({ queryKey: ['ref-schemas', source.key], queryFn: () => source.schemas(), enabled: isNew })
  const pk = draft.primaryKey
  const props = [
    storage?.engine && t('design.option.engine', { value: storage.engine }),
    storage?.charset && t('design.option.charset', { value: storage.charset }),
    storage?.collation && t('design.option.collation', { value: storage.collation }),
    storage?.tablespace && t('design.option.tablespace', { value: storage.tablespace }),
    storage?.clustered && t('design.option.clustered'),
  ].filter(Boolean)
  return (
    <div className="design-pane design-options">
      <label>
        {t('design.option.schema')}
        {isNew ? (
          <Cell error={schema ? undefined : t('design.option.schemaRequired')}>
            <Select size="small" showSearch value={schema || undefined} options={schemas.map((s) => ({ value: s }))} onChange={onSchema} />
          </Cell>
        ) : (
          <Input size="small" value={schema} disabled />
        )}
      </label>
      <label>
        {t('design.option.name')}
        <Cell error={issues.at('table', 'name')}>
          <Input
            size="small"
            value={draft.name}
            disabled={!isNew}
            title={isNew ? undefined : t('design.option.nameReadonly')}
            onChange={(e) => onChange((d) => ({ ...d, name: e.target.value }))}
          />
        </Cell>
      </label>
      <label>
        {t('design.option.comment')}
        <Input.TextArea rows={2} value={draft.comment ?? ''} onChange={(e) => onChange((d) => ({ ...d, comment: e.target.value || null }))} />
      </label>
      <label>
        {t('design.option.pkName')}
        <Input
          size="small"
          value={pk?.name ?? ''}
          disabled={!pk}
          placeholder={pk ? undefined : t('design.option.noPk')}
          onChange={(e) => onChange((d) => ({ ...d, primaryKey: d.primaryKey && { ...d.primaryKey, name: e.target.value || null } }))}
        />
      </label>
      <label>
        {t('design.option.pkColumns')}
        <Cell error={issues.at('primary_key', 'columns')}>
          <div className="design-pk">
            {pk ? (
              pk.columns.map((c, i) => (
                <div key={c} className="row">
                  <span className="grow">{c}</span>
                  <Button size="small" type="text" icon={<IconArrowUp size={14} />} disabled={i === 0} onClick={() => onChange((d) => ({ ...d, primaryKey: d.primaryKey && { ...d.primaryKey, columns: moveItem(d.primaryKey.columns, i, -1) } }))} />
                  <Button size="small" type="text" icon={<IconArrowDown size={14} />} disabled={i === pk.columns.length - 1} onClick={() => onChange((d) => ({ ...d, primaryKey: d.primaryKey && { ...d.primaryKey, columns: moveItem(d.primaryKey.columns, i, 1) } }))} />
                </div>
              ))
            ) : (
              <span className="muted">{t('design.option.noPk')}</span>
            )}
          </div>
        </Cell>
      </label>
      <label>
        {t('design.option.partition')}
        <span className={draft.partition ? '' : 'muted'}>{draft.partition ?? t('design.option.noPartition')}</span>
      </label>
      {!isNew && (
        <label>
          {t('design.option.rows')}
          <span>{storage?.row_count != null ? storage.row_count.toLocaleString() : t('design.option.rowsUnknown')}</span>
        </label>
      )}
      {props.length > 0 && (
        <label>
          {t('design.option.storage')}
          <span>{props.join(' · ')}</span>
        </label>
      )}
    </div>
  )
}
