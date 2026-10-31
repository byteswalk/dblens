// 账号与权限的修改（M9 设计文档第 9 节）：表单收集内容，统一进预览对话框，确认后执行。
// 密码只放在组件状态里：不进 tabs、localStorage 和草稿，对话框关闭就没了。

import { Alert, Button, Checkbox, Input, Modal, Select, Space } from 'antd'
import { useEffect, useState } from 'react'
import { useTranslation } from 'react-i18next'

import { useApi } from '../../app/context'
import type { GrantScope, Principal, PrivApplyResult, PrivCaps, PrivChange, PrivPreview } from '../../shared/api'
import { principalRef, randomPassword, withCascade } from './logic'

const SQL_STYLE = { whiteSpace: 'pre-wrap', margin: '0 0 8px', fontFamily: 'var(--mono)' } as const

export function ChangeDialog({ profileId, catalog, change, cascadable, onClose, onDone }: {
  profileId: string
  catalog: string | undefined
  change: PrivChange
  cascadable: boolean
  onClose: () => void
  onDone: () => void
}) {
  const api = useApi()
  const { t } = useTranslation()
  const [cascade, setCascade] = useState(false)
  const [preview, setPreview] = useState<PrivPreview | null>(null)
  const [error, setError] = useState<string | null>(null)
  const [typed, setTyped] = useState('')
  const [running, setRunning] = useState(false)
  const [result, setResult] = useState<PrivApplyResult | null>(null)
  const current = cascadable ? withCascade(change, cascade) : change

  useEffect(() => {
    let live = true
    setPreview(null)
    setError(null)
    api.privilegePreview(profileId, current, catalog).then(
      (p) => live && setPreview(p),
      (e: Error) => live && setError(e.message),
    )
    return () => {
      live = false
    }
  }, [api, profileId, catalog, cascade]) // eslint-disable-line react-hooks/exhaustive-deps

  const blocked = !preview || (preview.confirm_name != null && typed !== preview.confirm_name)
  const run = async () => {
    setRunning(true)
    setError(null)
    try {
      const r = await api.privilegeApply(profileId, current, catalog, preview?.confirm_name ? typed : undefined)
      setResult(r)
      onDone()
    } catch (e) {
      setError((e as Error).message)
    } finally {
      setRunning(false)
    }
  }
  const finished = result !== null

  return (
    <Modal
      open
      width={640}
      title={t('privilege.change.title')}
      onCancel={onClose}
      destroyOnHidden
      footer={finished ? [<Button key="close" type="primary" onClick={onClose}>{t('privilege.change.close')}</Button>] : [
        <Button key="cancel" onClick={onClose}>{t('privilege.change.cancel')}</Button>,
        <Button key="run" type="primary" disabled={blocked} loading={running} onClick={() => void run()}>{t('privilege.change.run')}</Button>,
      ]}
    >
      {error && <Alert type="error" showIcon message={error} style={{ marginBottom: 8 }} />}
      {preview && !finished && (
        <>
          <div className="muted" style={{ marginBottom: 4 }}>{t('privilege.change.connection', { name: preview.profile_name })}</div>
          <pre style={SQL_STYLE}>{preview.statements.join(';\n')}</pre>
          {preview.statements.length > 1 && <div className="muted" style={{ marginBottom: 8 }}>{t('privilege.change.notAtomic')}</div>}
          {preview.own_account && <Alert type="warning" showIcon message={t('privilege.change.ownAccount')} style={{ marginBottom: 8 }} />}
          {preview.confirm.map((m) => <Alert key={m} type="warning" showIcon message={m} style={{ marginBottom: 8 }} />)}
          {preview.owned.length > 0 && (
            <div className="muted" style={{ marginBottom: 8 }}>{t('privilege.change.owned', { list: preview.owned.join(', ') })}</div>
          )}
          {cascadable && <Checkbox checked={cascade} onChange={(e) => setCascade(e.target.checked)}>{t('privilege.change.cascade')}</Checkbox>}
          {preview.confirm_name != null && (
            <div style={{ marginTop: 8 }}>
              <div style={{ marginBottom: 4 }}>{t('privilege.change.typeName', { name: preview.confirm_name })}</div>
              <Input value={typed} onChange={(e) => setTyped(e.target.value)} autoComplete="off" />
            </div>
          )}
        </>
      )}
      {result && (
        <>
          {result.done.length > 0 && (
            <>
              <div style={{ marginBottom: 4 }}>{t('privilege.change.done')}</div>
              <pre style={SQL_STYLE}>{result.done.join(';\n')}</pre>
            </>
          )}
          {result.error && <Alert type="error" showIcon message={t('privilege.change.failed', { statement: result.failed })} description={result.error} />}
        </>
      )}
    </Modal>
  )
}

function PasswordField({ value, onChange }: { value: string; onChange: (v: string) => void }) {
  const { t } = useTranslation()
  const [shown, setShown] = useState(false)
  return (
    <Space.Compact style={{ width: '100%' }}>
      {shown ? (
        <Input value={value} onChange={(e) => onChange(e.target.value)} autoComplete="new-password" />
      ) : (
        <Input.Password value={value} onChange={(e) => onChange(e.target.value)} autoComplete="new-password" visibilityToggle={false} />
      )}
      <Button
        onClick={() => {
          onChange(randomPassword())
          setShown(true)
        }}
      >
        {t('privilege.action.generate')}
      </Button>
    </Space.Compact>
  )
}

function Field({ label, children }: { label: string; children: React.ReactNode }) {
  return (
    <div style={{ marginBottom: 10 }}>
      <div style={{ marginBottom: 4 }}>{label}</div>
      {children}
    </div>
  )
}

function FormDialog({ title, disabled, onCancel, onNext, children }: {
  title: string
  disabled: boolean
  onCancel: () => void
  onNext: () => void
  children: React.ReactNode
}) {
  const { t } = useTranslation()
  return (
    <Modal open title={title} onCancel={onCancel} destroyOnHidden okText={t('privilege.action.next')} cancelText={t('privilege.change.cancel')} okButtonProps={{ disabled }} onOk={onNext}>
      {children}
    </Modal>
  )
}

export function CreateDialog({ role, caps, onCancel, onNext }: {
  role: boolean
  caps: PrivCaps
  onCancel: () => void
  onNext: (c: PrivChange) => void
}) {
  const { t } = useTranslation()
  const [name, setName] = useState('')
  const [host, setHost] = useState('%')
  const [password, setPassword] = useState('')
  const needsPassword = !role || caps.role_password
  const next = () =>
    onNext(role
      ? { op: 'create_role', name, level: 'database', password: caps.role_password ? password : undefined }
      : { op: 'create_user', name, host: caps.host ? host : undefined, password })
  return (
    <FormDialog title={t(role ? 'privilege.action.newRole' : 'privilege.action.newUser')} disabled={!name.trim() || (needsPassword && !password)} onCancel={onCancel} onNext={next}>
      <Field label={t('privilege.attr.name')}><Input autoFocus value={name} onChange={(e) => setName(e.target.value)} autoComplete="off" /></Field>
      {caps.host && !role && <Field label={t('privilege.attr.host')}><Input value={host} onChange={(e) => setHost(e.target.value)} /></Field>}
      {needsPassword && <Field label={t('privilege.action.password')}><PasswordField value={password} onChange={setPassword} /></Field>}
    </FormDialog>
  )
}

export function PasswordDialog({ principal, onCancel, onNext }: { principal: Principal; onCancel: () => void; onNext: (c: PrivChange) => void }) {
  const { t } = useTranslation()
  const [password, setPassword] = useState('')
  return (
    <FormDialog title={t('privilege.action.setPassword')} disabled={!password} onCancel={onCancel} onNext={() => onNext({ op: 'set_password', principal: principalRef(principal), password })}>
      <Field label={t('privilege.action.password')}><PasswordField value={password} onChange={setPassword} /></Field>
    </FormDialog>
  )
}

export function GrantDialog({ principal, caps, catalog, onCancel, onNext }: {
  principal: Principal
  caps: PrivCaps
  catalog: string | undefined
  onCancel: () => void
  onNext: (c: PrivChange) => void
}) {
  const { t } = useTranslation()
  const scopes: GrantScope[] = [
    'object',
    'column',
    ...(caps.schema_grant ? (['schema'] as const) : []),
    ...(caps.database_grant ? (['database'] as const) : []),
    ...(caps.global_grant ? (['global'] as const) : []),
  ]
  const [scope, setScope] = useState<GrantScope>('object')
  const [schema, setSchema] = useState('')
  const [object, setObject] = useState('')
  const [column, setColumn] = useState('')
  const [database, setDatabase] = useState(catalog ?? '')
  const [privilege, setPrivilege] = useState('SELECT')
  const [withGrant, setWithGrant] = useState(false)
  const hasSchema = scope === 'schema' || scope === 'object' || scope === 'column'
  const hasObject = scope === 'object' || scope === 'column'
  const missing = !privilege.trim() || (hasSchema && !schema.trim()) || (hasObject && !object.trim()) || (scope === 'column' && !column.trim())
  const next = () =>
    onNext({
      op: 'grant',
      grantee: principalRef(principal),
      scope,
      target: {
        database: scope === 'database' && database ? database : null,
        schema: hasSchema ? schema : null,
        object_kind: hasObject ? 'table' : null,
        object: hasObject ? object : null,
        column: scope === 'column' ? column : null,
      },
      privilege,
      with_grant: withGrant,
    })
  return (
    <FormDialog title={t('privilege.action.grant')} disabled={missing} onCancel={onCancel} onNext={next}>
      <Field label={t('privilege.col.scope')}>
        <Select style={{ width: '100%' }} value={scope} onChange={setScope} options={scopes.map((s) => ({ value: s, label: t(`privilege.scope.${s}`) }))} />
      </Field>
      {scope === 'database' && <Field label={t('privilege.scope.database')}><Input value={database} onChange={(e) => setDatabase(e.target.value)} /></Field>}
      {hasSchema && <Field label={t('privilege.scope.schema')}><Input value={schema} onChange={(e) => setSchema(e.target.value)} /></Field>}
      {hasObject && <Field label={t('privilege.scope.object')}><Input value={object} onChange={(e) => setObject(e.target.value)} /></Field>}
      {scope === 'column' && <Field label={t('privilege.scope.column')}><Input value={column} onChange={(e) => setColumn(e.target.value)} /></Field>}
      <Field label={t('privilege.col.privilege')}><Input value={privilege} onChange={(e) => setPrivilege(e.target.value)} /></Field>
      <Checkbox checked={withGrant} onChange={(e) => setWithGrant(e.target.checked)}>{t('privilege.action.withGrant')}</Checkbox>
    </FormDialog>
  )
}

export function AddRoleDialog({ principal, roles, onCancel, onNext }: {
  principal: Principal
  roles: Principal[]
  onCancel: () => void
  onNext: (c: PrivChange) => void
}) {
  const { t } = useTranslation()
  const [id, setId] = useState<string>()
  const [admin, setAdmin] = useState(false)
  const role = roles.find((r) => r.id === id)
  return (
    <FormDialog
      title={t('privilege.action.addRole')}
      disabled={!role}
      onCancel={onCancel}
      onNext={() => role && onNext({ op: 'add_role', member: principalRef(principal), role: principalRef(role), admin })}
    >
      <Field label={t('privilege.kind.role')}>
        <Select showSearch optionFilterProp="label" style={{ width: '100%' }} value={id} onChange={setId} options={roles.map((r) => ({ value: r.id, label: r.name }))} />
      </Field>
      <Checkbox checked={admin} onChange={(e) => setAdmin(e.target.checked)}>{t('privilege.action.withAdmin')}</Checkbox>
    </FormDialog>
  )
}
