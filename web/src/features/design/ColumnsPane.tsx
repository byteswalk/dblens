// "列"子标签（M3 设计文档 7.3 节）：行内编辑名称、类型（按方言的下拉，可自由输入）、长度、小数位、非空、主键、自增、默认值、注释。
// 选中一行时下方显示生成列表达式和原生类型全文。新增、删除、上移、下移（Alt+↑/↓）、还原此行。

import { IconArrowBackUp, IconArrowDown, IconArrowUp, IconPlus, IconTrash } from '@tabler/icons-react'
import { AutoComplete, Button, Checkbox, Input, Select, Table, Tooltip } from 'antd'
import type { KeyboardEvent } from 'react'
import { useTranslation } from 'react-i18next'

import type { DesignCaps } from '../../shared/api'
import {
  type Draft,
  type DraftColumn,
  type Issues,
  addColumn,
  changedFields,
  columnState,
  emptyColumn,
  findType,
  joinType,
  moveColumn,
  removeColumn,
  restoreColumn,
  splitType,
  togglePrimaryKey,
  typeParams,
  updateColumn,
} from './logic'

interface Props {
  draft: Draft
  original: Draft | null
  caps: DesignCaps | undefined
  issues: Issues
  selected: string | null
  onSelect(key: string | null): void
  onChange(fn: (d: Draft) => Draft): void
}

/** 带错误提示的单元格：出错时标红，悬停显示原因。 */
export function Cell({ error, children }: { error?: string; children: React.ReactNode }) {
  return error ? (
    <Tooltip title={error} color="red">
      <div className="design-cell-error">{children}</div>
    </Tooltip>
  ) : (
    <>{children}</>
  )
}

export default function ColumnsPane({ draft, original, caps, issues, selected, onSelect, onChange }: Props) {
  const { t } = useTranslation()
  const types = caps?.types ?? []
  const pk = draft.primaryKey?.columns ?? []
  const sel = draft.columns.find((c) => c.key === selected)
  const set = (key: string, patch: Partial<DraftColumn>) => onChange((d) => updateColumn(d, key, patch))

  const add = () => {
    const c = emptyColumn()
    // 不能指定列位置的库：新列加在最后（插在中间要重建表）
    onChange((d) => addColumn(d, c, caps?.reorder ? (selected ?? undefined) : undefined))
    onSelect(c.key)
  }
  const move = (delta: -1 | 1) => sel && onChange((d) => moveColumn(d, sel.key, delta))
  const onKeyDown = (e: KeyboardEvent) => {
    if (e.altKey && (e.key === 'ArrowUp' || e.key === 'ArrowDown')) {
      e.preventDefault()
      move(e.key === 'ArrowUp' ? -1 : 1)
    }
  }

  const typeCell = (c: DraftColumn) => {
    const parts = splitType(c.data_type.native)
    const entry = findType(parts.name, types)
    const params = typeParams(parts.name, types)
    const native = (p: Partial<typeof parts>) => set(c.key, { data_type: { ...c.data_type, native: joinType({ ...parts, ...p }) } })
    return { parts, entry, params, native }
  }

  const columns = [
    {
      title: t('design.column.name'),
      width: 180,
      render: (_: unknown, c: DraftColumn) => (
        <Cell error={issues.at(c.key, 'name')}>
          <Input size="small" variant="borderless" value={c.name} disabled={c.deleted} onChange={(e) => set(c.key, { name: e.target.value })} />
        </Cell>
      ),
    },
    {
      title: t('design.column.type'),
      width: 190,
      render: (_: unknown, c: DraftColumn) => {
        const { parts, native } = typeCell(c)
        return (
          <Cell error={issues.at(c.key, 'type')}>
            <AutoComplete
              size="small"
              variant="borderless"
              style={{ width: '100%' }}
              value={parts.name}
              disabled={c.deleted}
              options={types.map((x) => ({ value: x.name }))}
              filterOption={(input, o) => String(o?.value ?? '').toLowerCase().includes(input.toLowerCase())}
              onChange={(v: string) => native({ name: v })}
            />
          </Cell>
        )
      },
    },
    {
      title: t('design.column.length'),
      width: 120,
      render: (_: unknown, c: DraftColumn) => {
        const { parts, entry, params, native } = typeCell(c)
        return (
          <div className="row" style={{ gap: 4 }}>
            <Input
              size="small"
              variant="borderless"
              value={parts.length}
              disabled={c.deleted || params === 'none'}
              onChange={(e) => native({ length: e.target.value.replace(/[^\d]/g, '') })}
            />
            {entry?.char_unit && (
              <Select
                size="small"
                variant="borderless"
                style={{ width: 70 }}
                value={parts.unit || ''}
                disabled={c.deleted}
                options={[
                  { value: '', label: '-' },
                  { value: 'BYTE', label: 'BYTE' },
                  { value: 'CHAR', label: 'CHAR' },
                ]}
                onChange={(unit: '' | 'BYTE' | 'CHAR') => native({ unit })}
              />
            )}
          </div>
        )
      },
    },
    {
      title: t('design.column.scale'),
      width: 72,
      render: (_: unknown, c: DraftColumn) => {
        const { parts, params, native } = typeCell(c)
        return (
          <Input
            size="small"
            variant="borderless"
            value={parts.scale}
            disabled={c.deleted || params !== 'precision_scale'}
            onChange={(e) => native({ scale: e.target.value.replace(/[^\d-]/g, '') })}
          />
        )
      },
    },
    {
      title: t('design.column.notNull'),
      width: 56,
      align: 'center' as const,
      render: (_: unknown, c: DraftColumn) => (
        <Checkbox checked={!c.nullable} disabled={c.deleted || pk.includes(c.name)} onChange={(e) => set(c.key, { nullable: !e.target.checked })} />
      ),
    },
    {
      title: t('design.column.primaryKey'),
      width: 56,
      align: 'center' as const,
      render: (_: unknown, c: DraftColumn) => (
        <Checkbox checked={pk.includes(c.name)} disabled={c.deleted || !c.name} onChange={() => onChange((d) => togglePrimaryKey(d, c.name))} />
      ),
    },
    {
      title: t('design.column.autoIncrement'),
      width: 56,
      align: 'center' as const,
      render: (_: unknown, c: DraftColumn) => (
        <Checkbox checked={c.auto_increment} disabled={c.deleted} onChange={(e) => set(c.key, { auto_increment: e.target.checked })} />
      ),
    },
    {
      title: (
        <Tooltip title={t('design.column.defaultHint')}>
          <span>{t('design.column.default')}</span>
        </Tooltip>
      ),
      width: 150,
      render: (_: unknown, c: DraftColumn) => (
        <Input
          size="small"
          variant="borderless"
          value={c.default ?? ''}
          disabled={c.deleted || c.auto_increment || !!c.generated}
          onChange={(e) => set(c.key, { default: e.target.value || null })}
        />
      ),
    },
    {
      title: t('design.column.comment'),
      render: (_: unknown, c: DraftColumn) => (
        <Input size="small" variant="borderless" value={c.comment ?? ''} disabled={c.deleted} onChange={(e) => set(c.key, { comment: e.target.value || null })} />
      ),
    },
  ]

  const state = sel ? columnState(sel, original) : null
  return (
    <div className="design-pane" tabIndex={-1} onKeyDown={onKeyDown}>
      <div className="row toolbar">
        <Button size="small" type="text" icon={<IconPlus size={16} />} onClick={add}>
          {t('design.column.add')}
        </Button>
        <Button size="small" type="text" icon={<IconTrash size={16} />} disabled={!sel || sel.deleted} onClick={() => sel && onChange((d) => removeColumn(d, sel.key))}>
          {t('design.remove')}
        </Button>
        <Tooltip title="Alt+↑">
          <Button size="small" type="text" icon={<IconArrowUp size={16} />} disabled={!sel} onClick={() => move(-1)}>
            {t('design.column.moveUp')}
          </Button>
        </Tooltip>
        <Tooltip title="Alt+↓">
          <Button size="small" type="text" icon={<IconArrowDown size={16} />} disabled={!sel} onClick={() => move(1)}>
            {t('design.column.moveDown')}
          </Button>
        </Tooltip>
        <Button
          size="small"
          type="text"
          icon={<IconArrowBackUp size={16} />}
          disabled={!sel || !original || (state !== 'changed' && state !== 'deleted')}
          onClick={() => sel && original && onChange((d) => restoreColumn(d, sel.key, original))}
        >
          {t('design.restore')}
        </Button>
        {caps && !caps.reorder && original && <span className="muted">{t('design.column.reorderNote')}</span>}
      </div>
      <Table<DraftColumn>
        size="small"
        rowKey="key"
        className="design-table"
        pagination={false}
        dataSource={draft.columns}
        columns={columns}
        locale={{ emptyText: t('design.column.empty') }}
        scroll={{ y: 'calc(100vh - 380px)' }}
        rowClassName={(c) => {
          const s = columnState(c, original)
          return `${s ? `design-${s}` : ''}${c.key === selected ? ' design-selected' : ''}`
        }}
        onRow={(c) => ({ onClick: () => onSelect(c.key) })}
      />
      <div className="design-props">
        {sel ? (
          <>
            <label>
              {t('design.column.generated')}
              <Input
                size="small"
                value={sel.generated ?? ''}
                disabled={sel.deleted}
                placeholder={t('design.column.generatedHint')}
                onChange={(e) => set(sel.key, { generated: e.target.value || null })}
              />
            </label>
            <label>
              {t('design.column.native')}
              <code className={changedFields(sel, original).has('type') ? 'design-changed-text' : ''}>{sel.data_type.native || '-'}</code>
            </label>
          </>
        ) : (
          <span className="muted">{t('design.column.noSelection')}</span>
        )}
      </div>
    </div>
  )
}
