// 账号与权限标签（M9 设计文档第 8 节）：左侧主体列表，右侧属性、直接授权、实际权限；对象反查时右侧列出访问者。只读。

import { Alert, Button, Checkbox, Empty, Input, Segmented, Select, Spin, Table, Tabs, Tooltip } from 'antd'
import type { ColumnsType } from 'antd/es/table'
import { IconArrowLeft, IconRefresh } from '@tabler/icons-react'
import { useQuery, useQueryClient } from '@tanstack/react-query'
import { useMemo, useState } from 'react'
import { useTranslation } from 'react-i18next'

import { useApi } from '../../app/context'
import { schemaLabel, splitCatalog } from '../../shared/api'
import type { Coverage, EffectivePrivilege, Principal, PrivilegeGrant, PrivNote } from '../../shared/api'
import { type QueryTab, useTabs } from '../editor/tabs'
import { type KindFilter, filterPrincipals, groupAccess, pathLabel, targetLabel } from './logic'

function CoverageNote({ coverage }: { coverage: Coverage | undefined }) {
  const { t } = useTranslation()
  if (coverage?.state !== 'partial') return null
  return <Alert type="warning" showIcon banner message={`${t('privilege.partial')}：${coverage.missing.join('；')}`} />
}

function Notes({ notes }: { notes: PrivNote[] }) {
  const { t } = useTranslation()
  if (!notes.length) return null
  return <span className="muted"> {notes.map((n) => t(`privilege.note.${n}`)).join('；')}</span>
}

function useGrantColumns(): ColumnsType<PrivilegeGrant> {
  const { t } = useTranslation()
  return [
    { title: t('privilege.col.scope'), dataIndex: 'scope', width: 90, render: (s: PrivilegeGrant['scope']) => t(`privilege.scope.${s}`) },
    { title: t('privilege.col.target'), key: 'target', ellipsis: true, render: (_, g) => targetLabel(g) },
    { title: t('privilege.col.privilege'), dataIndex: 'privilege', width: 200 },
    { title: t('privilege.col.grantable'), dataIndex: 'grantable', width: 80, render: (v: boolean) => (v ? t('privilege.yes') : t('privilege.no')) },
    { title: t('privilege.col.grantor'), dataIndex: 'grantor', width: 140, ellipsis: true },
  ]
}

function useEffectiveColumns(principals: Principal[]): ColumnsType<EffectivePrivilege> {
  const { t } = useTranslation()
  return [
    { title: t('privilege.col.scope'), key: 'scope', width: 90, render: (_, e) => t(`privilege.scope.${e.grant.scope}`) },
    { title: t('privilege.col.target'), key: 'target', ellipsis: true, render: (_, e) => targetLabel(e.grant) },
    { title: t('privilege.col.privilege'), key: 'privilege', width: 180, render: (_, e) => e.grant.privilege },
    { title: t('privilege.col.state'), dataIndex: 'state', width: 80, render: (s: EffectivePrivilege['state']) => t(`privilege.state.${s}`) },
    { title: t('privilege.col.path'), key: 'path', render: (_, e) => <span>{pathLabel(e.via, principals)}<Notes notes={e.notes} /></span> },
  ]
}

function PrincipalDetail({ tab, catalog, principal, principals, roles }: {
  tab: QueryTab
  catalog: string | undefined
  principal: Principal
  principals: Principal[]
  roles: string[]
}) {
  const api = useApi()
  const { t } = useTranslation()
  const grantColumns = useGrantColumns()
  const effectiveColumns = useEffectiveColumns(principals)
  const { data, error, isLoading } = useQuery({
    queryKey: ['privileges', tab.profileId, catalog, 'principal', principal.id],
    queryFn: () => api.principalPrivileges(tab.profileId, principal.id, catalog),
  })
  const yesNo = (v: boolean | null) => (v === null ? t('privilege.unknown') : v ? t('privilege.yes') : t('privilege.no'))
  const rows: [string, string][] = [
    [t('privilege.attr.name'), principal.name],
    [t('privilege.attr.kind'), t(`privilege.kindName.${principal.kind}`)],
    [t('privilege.attr.level'), t(`privilege.levelName.${principal.level}`)],
    ...(principal.host ? ([[t('privilege.attr.host'), principal.host]] as [string, string][]) : []),
    [t('privilege.attr.login'), yesNo(principal.can_login)],
    [t('privilege.attr.locked'), yesNo(principal.locked)],
    [t('privilege.attr.expires'), principal.expires_at ?? '—'],
    [t('privilege.attr.roles'), roles.join(', ') || '—'],
    [t('privilege.attr.builtin'), principal.builtin ? t('privilege.yes') : t('privilege.no')],
    ...principal.attrs,
  ]
  return (
    <div style={{ display: 'flex', flexDirection: 'column', minHeight: 0, flex: 1 }}>
      <CoverageNote coverage={data?.coverage} />
      {error && <div className="err" style={{ padding: 12 }}>{t('privilege.error')}：{(error as Error).message}</div>}
      <Tabs
        size="small"
        style={{ padding: '0 12px', flex: 1, minHeight: 0, overflow: 'auto' }}
        items={[
          {
            key: 'attrs',
            label: t('privilege.tab.attrs'),
            children: (
              <table className="kv">
                <tbody>
                  {rows.map(([k, v]) => (
                    <tr key={k}><td className="muted" style={{ paddingRight: 24 }}>{k}</td><td>{v}</td></tr>
                  ))}
                </tbody>
              </table>
            ),
          },
          {
            key: 'direct',
            label: t('privilege.tab.direct'),
            children: <Table size="small" pagination={false} loading={isLoading} rowKey={(g) => `${g.scope}|${targetLabel(g)}|${g.privilege}`} columns={grantColumns} dataSource={data?.direct} />,
          },
          {
            key: 'effective',
            label: t('privilege.tab.effective'),
            children: (
              <>
                <div className="muted" style={{ marginBottom: 8 }}>{t('privilege.computed')}</div>
                <Table size="small" pagination={false} loading={isLoading} rowKey={(e) => `${e.via.join('>')}|${e.grant.scope}|${targetLabel(e.grant)}|${e.grant.privilege}`} columns={effectiveColumns} dataSource={data?.effective} />
              </>
            ),
          },
        ]}
      />
    </div>
  )
}

function ObjectAccessPanel({ tab, principals, onPick }: { tab: QueryTab; principals: Principal[]; onPick: (id: string) => void }) {
  const api = useApi()
  const { t } = useTranslation()
  const focus = tab.privileges!.focus!
  const { catalog, schema } = splitCatalog(focus.schema)
  const { data, error, isLoading } = useQuery({
    queryKey: ['privileges', tab.profileId, catalog, 'object', focus.schema, focus.object],
    queryFn: () => api.objectAccess(tab.profileId, schema, focus.object, catalog),
  })
  const groups = useMemo(() => groupAccess(data?.access ?? []), [data])
  return (
    <div style={{ display: 'flex', flexDirection: 'column', minHeight: 0, flex: 1 }}>
      <div className="row" style={{ padding: '6px 12px', gap: 8 }}>
        <Button size="small" type="text" icon={<IconArrowLeft size={14} />} onClick={() => useTabs.getState().setPrivilegesFocus(tab.id, undefined)}>
          {t('privilege.object.back')}
        </Button>
        <strong>{t('privilege.object.title', { target: `${schemaLabel(focus.schema)}.${focus.object}` })}</strong>
      </div>
      <CoverageNote coverage={data?.coverage} />
      <div className="muted" style={{ padding: '4px 12px' }}>{t('privilege.computed')}</div>
      {error && <div className="err" style={{ padding: 12 }}>{t('privilege.error')}：{(error as Error).message}</div>}
      <div style={{ flex: 1, overflow: 'auto', padding: '0 12px' }}>
        {isLoading ? <Spin style={{ margin: 48 }} /> : !groups.length ? (
          <Empty image={Empty.PRESENTED_IMAGE_SIMPLE} description={t('privilege.object.none')} />
        ) : (
          groups.map((g) => (
            <div key={g.principal} style={{ marginBottom: 12 }}>
              <a onClick={() => onPick(g.principal)}>{principals.find((p) => p.id === g.principal)?.name ?? g.principal}</a>
              {g.items.map((a, i) => (
                <div key={i} className="muted" style={{ paddingLeft: 12 }}>
                  {a.privilege.grant.privilege}（{t(`privilege.state.${a.privilege.state}`)}）· {pathLabel(a.privilege.via, principals)}
                  <Notes notes={a.privilege.notes} />
                </div>
              ))}
            </div>
          ))
        )}
      </div>
    </div>
  )
}

export default function PrivilegesView({ tab }: { tab: QueryTab }) {
  const api = useApi()
  const qc = useQueryClient()
  const { t } = useTranslation()
  const [text, setText] = useState('')
  const [kind, setKind] = useState<KindFilter>('all')
  const [builtin, setBuiltin] = useState(false)
  const [picked, setPicked] = useState<string | null>(null)
  const focus = tab.privileges?.focus
  const focusCatalog = focus ? splitCatalog(focus.schema).catalog : undefined
  const [chosenCatalog, setChosenCatalog] = useState<string | undefined>(undefined)
  const catalog = focusCatalog ?? chosenCatalog
  const { data: catalogs = [] } = useQuery({ queryKey: ['catalogs', tab.profileId], queryFn: () => api.catalogs(tab.profileId) })
  const { data, error, isLoading } = useQuery({
    queryKey: ['privileges', tab.profileId, catalog, 'overview'],
    queryFn: () => api.privilegeOverview(tab.profileId, catalog),
  })
  const principals = useMemo(() => data?.principals ?? [], [data])
  const list = useMemo(() => filterPrincipals(principals, { text, kind, builtin }), [principals, text, kind, builtin])
  const current = principals.find((p) => p.id === picked)
  const roles = (id: string) =>
    (data?.memberships ?? [])
      .filter((m) => m.member === id)
      .map((m) => principals.find((p) => p.id === m.role)?.name ?? m.role)

  return (
    <div style={{ display: 'flex', flex: 1, minHeight: 0 }}>
      <div style={{ width: 260, display: 'flex', flexDirection: 'column', borderRight: '1px solid var(--border)' }}>
        <div style={{ padding: 8, display: 'flex', flexDirection: 'column', gap: 6 }}>
          {catalogs.length > 0 && (
            <Select
              size="small"
              placeholder={t('privilege.catalog')}
              value={catalog}
              onChange={setChosenCatalog}
              options={catalogs.map((c) => ({ value: c.name, label: c.name }))}
            />
          )}
          <Input size="small" allowClear placeholder={t('privilege.search')} value={text} onChange={(e) => setText(e.target.value)} />
          <div className="row" style={{ gap: 6 }}>
            <Segmented
              size="small"
              value={kind}
              onChange={(v) => setKind(v as KindFilter)}
              options={[
                { value: 'all', label: t('privilege.kind.all') },
                { value: 'user', label: t('privilege.kind.user') },
                { value: 'role', label: t('privilege.kind.role') },
              ]}
            />
            <span className="grow" />
            <Tooltip title={t('privilege.refresh')}>
              <Button size="small" type="text" icon={<IconRefresh size={14} />} onClick={() => void qc.invalidateQueries({ queryKey: ['privileges', tab.profileId] })} />
            </Tooltip>
          </div>
          <Checkbox checked={builtin} onChange={(e) => setBuiltin(e.target.checked)}>{t('privilege.showBuiltin')}</Checkbox>
        </div>
        <div style={{ flex: 1, overflow: 'auto' }}>
          {isLoading && <Spin style={{ margin: 24 }} />}
          {!isLoading && !list.length && <Empty image={Empty.PRESENTED_IMAGE_SIMPLE} description={t('privilege.none')} />}
          {list.map((p) => (
            <div
              key={p.id}
              className="hist-item"
              style={{ padding: '4px 12px', cursor: 'pointer', fontWeight: p.id === picked ? 600 : undefined }}
              onClick={() => {
                setPicked(p.id)
                useTabs.getState().setPrivilegesFocus(tab.id, undefined)
              }}
            >
              {p.name}
              {p.host && <span className="muted">@{p.host}</span>}
              <span className="muted" style={{ marginLeft: 6, fontSize: 'var(--fs-xs)' }}>{t(`privilege.kindName.${p.kind}`)}</span>
            </div>
          ))}
        </div>
      </div>
      <div style={{ flex: 1, minWidth: 0, display: 'flex', flexDirection: 'column' }}>
        <CoverageNote coverage={data?.coverage} />
        {error && <div className="err" style={{ padding: 12 }}>{t('privilege.error')}：{(error as Error).message}</div>}
        {focus ? (
          <ObjectAccessPanel tab={tab} principals={principals} onPick={(id) => { setPicked(id); useTabs.getState().setPrivilegesFocus(tab.id, undefined) }} />
        ) : current ? (
          <PrincipalDetail key={current.id} tab={tab} catalog={catalog} principal={current} principals={principals} roles={roles(current.id)} />
        ) : (
          <Empty style={{ marginTop: 64 }} image={Empty.PRESENTED_IMAGE_SIMPLE} description={t('privilege.empty')} />
        )}
      </div>
    </div>
  )
}
