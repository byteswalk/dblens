// 账号与权限标签（M9 设计文档第 8、9 节）：左侧主体列表，右侧属性、直接授权、实际权限；对象反查时右侧列出访问者。
// 修改都经过 ChangeDialog 预览确认。

import { Alert, Button, Checkbox, Dropdown, Empty, Input, Segmented, Select, Spin, Table, Tabs, Tooltip } from 'antd'
import type { ColumnsType } from 'antd/es/table'
import { IconArrowLeft, IconPlus, IconRefresh } from '@tabler/icons-react'
import { useQuery, useQueryClient } from '@tanstack/react-query'
import { useEffect, useMemo, useState } from 'react'
import { useTranslation } from 'react-i18next'

import { useApi } from '../../app/context'
import { schemaLabel, splitCatalog } from '../../shared/api'
import type { Coverage, EffectivePrivilege, Membership, Principal, PrivCaps, PrivChange, PrivilegeGrant, PrivNote } from '../../shared/api'
import { type QueryTab, useTabs } from '../editor/tabs'
import { AddRoleDialog, ChangeDialog, CreateDialog, GrantDialog, PasswordDialog } from './PrivilegeDialogs'
import { type KindFilter, filterPrincipals, groupAccess, hasCredentials, pathLabel, principalRef, targetLabel } from './logic'

type FormKind = 'new_user' | 'new_role' | 'password' | 'grant' | 'add_role'

function CoverageNote({ coverage }: { coverage: Coverage | undefined }) {
  const { t } = useTranslation()
  if (coverage?.state !== 'partial') return null
  return <Alert type="warning" showIcon banner message={t('privilege.partial', { detail: coverage.missing.join(' · ') })} />
}

function Notes({ notes }: { notes: PrivNote[] }) {
  const { t } = useTranslation()
  if (!notes.length) return null
  return <span className="muted"> {notes.map((n) => t(`privilege.note.${n}`)).join(' · ')}</span>
}

function useGrantColumns(): ColumnsType<PrivilegeGrant> {
  const { t } = useTranslation()
  return [
    { title: t('privilege.col.scope'), dataIndex: 'scope', width: 90, render: (s: PrivilegeGrant['scope']) => t(`privilege.scope.${s}`) },
    { title: t('privilege.col.target'), key: 'target', width: 220, ellipsis: true, render: (_, g) => targetLabel(g) },
    { title: t('privilege.col.privilege'), dataIndex: 'privilege', width: 200 },
    { title: t('privilege.col.grantable'), dataIndex: 'grantable', width: 80, render: (v: boolean) => (v ? t('privilege.yes') : t('privilege.no')) },
    { title: t('privilege.col.grantor'), dataIndex: 'grantor', width: 140, ellipsis: true },
  ]
}

function useEffectiveColumns(principals: Principal[]): ColumnsType<EffectivePrivilege> {
  const { t } = useTranslation()
  return [
    { title: t('privilege.col.scope'), key: 'scope', width: 90, render: (_, e) => t(`privilege.scope.${e.grant.scope}`) },
    { title: t('privilege.col.target'), key: 'target', width: 200, ellipsis: true, render: (_, e) => targetLabel(e.grant) },
    { title: t('privilege.col.privilege'), key: 'privilege', width: 160, render: (_, e) => e.grant.privilege },
    { title: t('privilege.col.state'), dataIndex: 'state', width: 80, render: (s: EffectivePrivilege['state']) => t(`privilege.state.${s}`) },
    { title: t('privilege.col.path'), key: 'path', width: 420, render: (_, e) => <span>{pathLabel(e.via, principals)}<Notes notes={e.notes} /></span> },
  ]
}

function PrincipalDetail({ tab, catalog, principal, principals, memberships, caps, onForm, onChange }: {
  tab: QueryTab
  catalog: string | undefined
  principal: Principal
  principals: Principal[]
  memberships: Membership[]
  caps: PrivCaps | undefined
  onForm: (f: FormKind) => void
  onChange: (c: PrivChange) => void
}) {
  const api = useApi()
  const { t } = useTranslation()
  const grantColumns = useGrantColumns()
  const effectiveColumns = useEffectiveColumns(principals)
  const { data, error, isLoading } = useQuery({
    queryKey: ['privileges', tab.profileId, catalog, 'principal', principal.id],
    queryFn: () => api.principalPrivileges(tab.profileId, principal.id, catalog),
  })
  const nameOf = (id: string) => principals.find((p) => p.id === id)?.name ?? id
  const memberOf = memberships.filter((m) => m.member === principal.id)
  const ref = principalRef(principal)
  const revokeColumn: ColumnsType<PrivilegeGrant>[number] = {
    key: 'revoke',
    width: 70,
    render: (_, g) =>
      g.effect === 'implicit' || !caps?.write ? null : (
        <a onClick={() => onChange({ op: 'revoke', grantee: ref, scope: g.scope, target: g.target, privilege: g.privilege, grantable: g.grantable, grantor: g.grantor })}>
          {t('privilege.action.revoke')}
        </a>
      ),
  }
  const removeRole = (m: Membership) => {
    const role = principals.find((p) => p.id === m.role)
    if (role) onChange({ op: 'remove_role', member: ref, role: principalRef(role) })
  }
  const setDefault = (m: Membership) => {
    const role = principals.find((p) => p.id === m.role)
    if (role) onChange({ op: 'set_default_role', member: ref, role: m.default_role ? null : principalRef(role) })
  }
  const yesNo = (v: boolean | null) => (v === null ? t('privilege.unknown') : v ? t('privilege.yes') : t('privilege.no'))
  const rows: [string, string][] = [
    [t('privilege.attr.name'), principal.name],
    [t('privilege.attr.kind'), t(`privilege.kindName.${principal.kind}`)],
    [t('privilege.attr.level'), t(`privilege.levelName.${principal.level}`)],
    ...(principal.host ? ([[t('privilege.attr.host'), principal.host]] as [string, string][]) : []),
    [t('privilege.attr.login'), yesNo(principal.can_login)],
    [t('privilege.attr.locked'), yesNo(principal.locked)],
    [t('privilege.attr.expires'), principal.expires_at ?? '—'],
    [t('privilege.attr.builtin'), principal.builtin ? t('privilege.yes') : t('privilege.no')],
    ...principal.attrs,
  ]
  return (
    <div style={{ display: 'flex', flexDirection: 'column', minHeight: 0, flex: 1 }}>
      <CoverageNote coverage={data?.coverage} />
      {error && <div className="err" style={{ padding: 12 }}>{t('privilege.error', { detail: (error as Error).message })}</div>}
      {caps?.write && <div className="row" style={{ padding: '6px 12px 0', gap: 2 }}>
        {hasCredentials(principal) && <Button size="small" type="text" onClick={() => onForm('password')}>{t('privilege.action.setPassword')}</Button>}
        {hasCredentials(principal) && caps?.lock && (
          <Button size="small" type="text" onClick={() => onChange({ op: 'set_locked', principal: ref, locked: !principal.locked })}>
            {t(principal.locked ? 'privilege.action.unlock' : 'privilege.action.lock')}
          </Button>
        )}
        <Button size="small" type="text" onClick={() => onForm('grant')}>{t('privilege.action.grant')}</Button>
        <Button size="small" type="text" onClick={() => onForm('add_role')}>{t('privilege.action.addRole')}</Button>
        <Button size="small" type="text" danger onClick={() => onChange({ op: 'drop', principal: ref })}>{t(caps.os_accounts && principal.kind === 'user' ? 'privilege.action.removeUser' : 'privilege.action.drop')}</Button>
      </div>}
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
                  <tr>
                    <td className="muted" style={{ paddingRight: 24 }}>{t('privilege.attr.roles')}</td>
                    <td>
                      {memberOf.length === 0 && '—'}
                      {memberOf.map((m) => (
                        <div key={m.role} className="row" style={{ gap: 8 }}>
                          {nameOf(m.role)}
                          {m.default_role && <span className="muted">{t('privilege.defaultRole')}</span>}
                          {caps?.write && <a onClick={() => removeRole(m)}>{t('privilege.action.removeRole')}</a>}
                          {caps?.os_accounts && <a onClick={() => setDefault(m)}>{t(m.default_role ? 'privilege.action.clearDefault' : 'privilege.action.setDefault')}</a>}
                        </div>
                      ))}
                    </td>
                  </tr>
                </tbody>
              </table>
            ),
          },
          {
            key: 'direct',
            label: t('privilege.tab.direct'),
            children: <Table size="small" pagination={false} loading={isLoading} scroll={{ x: 800 }} rowKey={(g) => `${g.scope}|${targetLabel(g)}|${g.privilege}`} columns={[...grantColumns, revokeColumn]} dataSource={data?.direct} />,
          },
          {
            key: 'effective',
            label: t('privilege.tab.effective'),
            children: (
              <>
                <div className="muted" style={{ marginBottom: 8 }}>{t('privilege.computed')}</div>
                <Table size="small" pagination={false} loading={isLoading} scroll={{ x: 950 }} rowKey={(e) => `${e.via.join('>')}|${e.grant.scope}|${targetLabel(e.grant)}|${e.grant.privilege}`} columns={effectiveColumns} dataSource={data?.effective} />
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
      {error && <div className="err" style={{ padding: 12 }}>{t('privilege.error', { detail: (error as Error).message })}</div>}
      <div style={{ flex: 1, overflow: 'auto', padding: '0 12px' }}>
        {isLoading ? <Spin style={{ margin: 48 }} /> : !groups.length ? (
          <Empty image={Empty.PRESENTED_IMAGE_SIMPLE} description={t('privilege.object.none')} />
        ) : (
          groups.map((g) => (
            <div key={g.principal} style={{ marginBottom: 12 }}>
              <a onClick={() => onPick(g.principal)}>{principals.find((p) => p.id === g.principal)?.name ?? g.principal}</a>
              {g.items.map((a, i) => (
                <div key={i} className="muted" style={{ paddingLeft: 12 }}>
                  {a.privilege.grant.privilege} · {t(`privilege.state.${a.privilege.state}`)} · {pathLabel(a.privilege.via, principals)}
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
  // 从“谁能访问”回到账号页时留在同一个数据库
  useEffect(() => {
    if (focusCatalog) setChosenCatalog(focusCatalog)
  }, [focusCatalog])
  const { data: catalogs = [] } = useQuery({ queryKey: ['catalogs', tab.profileId], queryFn: () => api.catalogs(tab.profileId) })
  const { data, error, isLoading } = useQuery({
    queryKey: ['privileges', tab.profileId, catalog, 'overview'],
    queryFn: () => api.privilegeOverview(tab.profileId, catalog),
  })
  const principals = useMemo(() => data?.principals ?? [], [data])
  const list = useMemo(() => filterPrincipals(principals, { text, kind, builtin }), [principals, text, kind, builtin])
  const current = principals.find((p) => p.id === picked)
  const [form, setForm] = useState<FormKind | null>(null)
  const [change, setChange] = useState<PrivChange | null>(null)
  const review = (c: PrivChange) => {
    setForm(null)
    setChange(c)
  }
  const refresh = () => void qc.invalidateQueries({ queryKey: ['privileges', tab.profileId] })

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
            {data?.caps.write && (
              <Dropdown
                trigger={['click']}
                menu={{
                  items: [
                    { key: 'new_user', label: t('privilege.action.newUser') },
                    { key: 'new_role', label: t('privilege.action.newRole') },
                  ],
                  onClick: ({ key }) => setForm(key as FormKind),
                }}
              >
                <Button size="small" type="text" aria-label={t('privilege.action.new')} icon={<IconPlus size={14} />} />
              </Dropdown>
            )}
            <Tooltip title={t('privilege.refresh')}>
              <Button size="small" type="text" icon={<IconRefresh size={14} />} onClick={refresh} />
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
        {error && <div className="err" style={{ padding: 12 }}>{t('privilege.error', { detail: (error as Error).message })}</div>}
        {focus ? (
          <ObjectAccessPanel tab={tab} principals={principals} onPick={(id) => { setPicked(id); useTabs.getState().setPrivilegesFocus(tab.id, undefined) }} />
        ) : current ? (
          <PrincipalDetail key={current.id} tab={tab} catalog={catalog} principal={current} principals={principals} memberships={data?.memberships ?? []} caps={data?.caps} onForm={setForm} onChange={setChange} />
        ) : (
          <Empty style={{ marginTop: 64 }} image={Empty.PRESENTED_IMAGE_SIMPLE} description={t('privilege.empty')} />
        )}
      </div>
      {data && (form === 'new_user' || form === 'new_role') && <CreateDialog role={form === 'new_role'} caps={data.caps} onCancel={() => setForm(null)} onNext={review} />}
      {current && form === 'password' && <PasswordDialog principal={current} onCancel={() => setForm(null)} onNext={review} />}
      {data && current && form === 'grant' && <GrantDialog principal={current} caps={data.caps} catalog={catalog} onCancel={() => setForm(null)} onNext={review} />}
      {current && form === 'add_role' && (
        <AddRoleDialog principal={current} roles={principals.filter((p) => p.kind === 'role' && p.id !== current.id && p.level === current.level)} onCancel={() => setForm(null)} onNext={review} />
      )}
      {change && (
        <ChangeDialog
          profileId={tab.profileId}
          catalog={catalog}
          change={change}
          cascadable={change.op === 'drop' && (data?.caps.drop_cascade ?? false) && change.principal.kind !== 'role'}
          onClose={() => setChange(null)}
          onDone={refresh}
        />
      )}
    </div>
  )
}
