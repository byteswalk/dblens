// "约束"子标签：唯一约束和检查约束两组，各自增、删、改。

import { Input, Select } from 'antd'
import { useTranslation } from 'react-i18next'

import type { DesignCaps } from '../../shared/api'
import { Cell } from './ColumnsPane'
import ListPane from './ListPane'
import { type Draft, type DraftCheck, type DraftKey, type Issues, newKey, updateItem } from './logic'

export default function ConstraintsPane({ draft, original, caps, issues, onChange }: {
  draft: Draft
  original: Draft | null
  caps: DesignCaps | undefined
  issues: Issues
  onChange(fn: (d: Draft) => Draft): void
}) {
  const { t } = useTranslation()
  const names = draft.columns.filter((c) => !c.deleted && c.name).map((c) => ({ value: c.name }))
  const setU = (key: string, patch: Partial<DraftKey>) => onChange((d) => updateItem(d, 'uniques', key, patch))
  const setC = (key: string, patch: Partial<DraftCheck>) => onChange((d) => updateItem(d, 'checks', key, patch))
  const nameCell = (key: string, name: string | null, deleted: boolean | undefined, set: (n: string | null) => void) => (
    <Cell error={issues.at(key, 'name')}>
      <Input size="small" variant="borderless" value={name ?? ''} disabled={deleted} onChange={(e) => set(e.target.value || null)} />
    </Cell>
  )
  return (
    <div className="design-pane">
      <h4>{t('design.constraint.uniques')}</h4>
      <ListPane
        list="uniques"
        draft={draft}
        original={original}
        onChange={onChange}
        addLabel={t('design.constraint.addUnique')}
        emptyText={t('design.constraint.noUniques')}
        create={() => ({ key: newKey(), origin: null, name: `uk_${draft.name || 't'}_${draft.uniques.length + 1}`, columns: [] })}
        columns={[
          { title: t('design.constraint.name'), width: 220, render: (_, u) => nameCell(u.key, u.name, u.deleted, (name) => setU(u.key, { name })) },
          {
            title: t('design.constraint.columns'),
            render: (_, u) => (
              <Cell error={issues.at(u.key, 'columns')}>
                <Select mode="multiple" size="small" variant="borderless" style={{ width: '100%' }} value={u.columns} options={names} disabled={u.deleted} onChange={(columns: string[]) => setU(u.key, { columns })} />
              </Cell>
            ),
          },
        ]}
      />
      <h4>{t('design.constraint.checks')}</h4>
      <ListPane
        list="checks"
        draft={draft}
        original={original}
        onChange={onChange}
        addLabel={t('design.constraint.addCheck')}
        emptyText={t('design.constraint.noChecks')}
        note={caps && !caps.checks ? <span className="muted">{t('design.constraint.checksIgnored')}</span> : undefined}
        create={() => ({ key: newKey(), origin: null, name: `ck_${draft.name || 't'}_${draft.checks.length + 1}`, expression: '' })}
        columns={[
          { title: t('design.constraint.name'), width: 220, render: (_, c) => nameCell(c.key, c.name, c.deleted, (name) => setC(c.key, { name })) },
          {
            title: t('design.constraint.expression'),
            render: (_, c) => (
              <Cell error={issues.at(c.key, 'expression')}>
                <Input size="small" variant="borderless" value={c.expression} disabled={c.deleted} onChange={(e) => setC(c.key, { expression: e.target.value })} />
              </Cell>
            ),
          },
        ]}
      />
    </div>
  )
}
