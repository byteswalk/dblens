// 新建 / 编辑连接（M1 设计稿第 2 部分）：左侧选数据库类型，右侧"常规 / SSL/TLS / 高级"三个标签。
// Oracle、达梦缺客户端库时，在对话框里直接确认目录。密码默认保存（系统钥匙串或主密码加密的本地密码库）。

import { Alert, App, Button, Checkbox, Form, Input, InputNumber, Modal, Radio, Space, Tabs } from 'antd'
import { IconCircleCheck, IconPlug } from '@tabler/icons-react'
import { useQuery, useQueryClient } from '@tanstack/react-query'
import { type ReactNode, useEffect, useState } from 'react'
import { useTranslation } from 'react-i18next'

import { useApi } from '../../app/context'
import { useUi } from '../../app/ui'
import DbIcon from '../../shared/DbIcon'
import type { DriverId, Environment, ServerSummary } from '../../shared/api'
import {
  type ConnectionForm,
  databaseLabel,
  driverLabel,
  driverNote,
  emptyForm,
  fromProfile,
  needsCredentials,
  serverText,
  supportsTls,
  serverNotes,
  switchProduct,
  toProfileInput,
  validate,
} from './form'
import { PRODUCT_GROUPS, product, productLabel, productNote } from './products'
import { withVault } from './vault'

type Errors = ReturnType<typeof validate>

function Field({ label, error, children }: { label: string; error?: string; children: ReactNode }) {
  return (
    <Form.Item label={label} validateStatus={error ? 'error' : undefined} help={error} style={{ marginBottom: 12 }}>
      {children}
    </Form.Item>
  )
}

const ENVS: Environment[] = ['none', 'dev', 'test', 'prod']

/** Oracle、达梦需要原生客户端库：未确认目录时提示，并可直接确认建议目录或填写其他目录。 */
function NativeLibNotice({ driver }: { driver: DriverId }) {
  const api = useApi()
  const qc = useQueryClient()
  const { t } = useTranslation()
  const { message } = App.useApp()
  const { data: drivers } = useQuery({ queryKey: ['drivers'], queryFn: () => api.drivers() })
  const status = drivers?.find((d) => d.id === driver)
  const [dir, setDir] = useState('')
  if (!status?.needs_native_lib || status.available) return null
  const confirm = async (path: string) => {
    try {
      await api.setLibDir(driver, path)
      await qc.invalidateQueries({ queryKey: ['drivers'] })
    } catch (e) {
      message.error((e as Error).message)
    }
  }
  return (
    <Alert
      type="warning"
      showIcon
      style={{ marginBottom: 12 }}
      message={status.hint ?? t('connections.nativeLib.needed', { name: status.name })}
      description={
        <Space direction="vertical" style={{ width: '100%' }}>
          {status.suggested_lib_dir && (
            <Button size="small" onClick={() => confirm(status.suggested_lib_dir!)}>
              {t('connections.nativeLib.useSuggested', { dir: status.suggested_lib_dir })}
            </Button>
          )}
          <Space.Compact style={{ width: '100%' }}>
            <Input size="small" placeholder={t('connections.nativeLib.otherDirPlaceholder')} value={dir} onChange={(e) => setDir(e.target.value)} />
            <Button size="small" disabled={!dir.trim()} onClick={() => confirm(dir.trim())}>
              {t('connections.nativeLib.useThisDir')}
            </Button>
          </Space.Compact>
        </Space>
      }
    />
  )
}

export default function ConnectionDialog() {
  const api = useApi()
  const qc = useQueryClient()
  const { t } = useTranslation()
  const { message } = App.useApp()
  const { connectionDialog, closeConnection } = useUi()
  const editingId = connectionDialog?.profileId ?? null
  const [form, setForm] = useState<ConnectionForm>(emptyForm())
  const [errors, setErrors] = useState<Errors>({})
  const [result, setResult] = useState<{ ok: ServerSummary } | { error: string } | null>(null)
  const [busy, setBusy] = useState<'test' | 'save' | null>(null)

  useEffect(() => {
    setErrors({})
    setResult(null)
    if (!connectionDialog) return
    if (editingId === null) {
      setForm(emptyForm())
      return
    }
    api.profile(editingId).then(
      (p) => setForm(fromProfile(p, true)),
      (e: Error) => message.error(e.message),
    )
  }, [connectionDialog, editingId, api, message])

  const set = <K extends keyof ConnectionForm>(k: K, v: ConnectionForm[K]) => setForm((f) => ({ ...f, [k]: v }))
  const check = () => {
    const e = validate(form)
    setErrors(e)
    return Object.keys(e).length === 0
  }

  const test = async () => {
    if (!check()) return
    setBusy('test')
    setResult(null)
    try {
      setResult({ ok: await api.testConnection(toProfileInput(form), form.password) })
    } catch (e) {
      setResult({ error: (e as Error).message })
    } finally {
      setBusy(null)
    }
  }

  const save = async () => {
    if (!check()) return
    setBusy('save')
    try {
      const input = toProfileInput(form)
      if (editingId === null) {
        const pw = form.savePassword && form.password ? form.password : null
        await withVault(api, () => api.createProfile(input, pw))
      } else {
        await api.updateProfile(editingId, input)
        // 编辑时密码留空表示沿用已保存的密码
        if (!form.savePassword) await withVault(api, () => api.setPassword(editingId, null))
        else if (form.password) await withVault(api, () => api.setPassword(editingId, form.password))
      }
      await qc.invalidateQueries({ queryKey: ['profiles'] })
      message.success(editingId === null ? t('connections.dialog.created') : t('connections.dialog.saved'))
      closeConnection()
    } catch (e) {
      message.error((e as Error).message)
    } finally {
      setBusy(null)
    }
  }

  const general = (
    <>
      <NativeLibNotice driver={form.driver} />
      <Field label={t('connections.general.name')} error={errors.name}>
        <Input value={form.name} onChange={(e) => set('name', e.target.value)} placeholder="prod-mysql" />
      </Field>
      <Field label={t('connections.general.hostPort')} error={errors.host ?? errors.port}>
        <Space.Compact style={{ width: '100%' }}>
          <Input value={form.host} onChange={(e) => set('host', e.target.value)} />
          <InputNumber style={{ width: 110 }} min={1} max={65535} value={form.port} onChange={(v) => set('port', v ?? 0)} />
        </Space.Compact>
      </Field>
      {form.driver === 'informix' && (
        <Field label={t('connections.gbase.serverName')} error={errors.serverName}>
          <Input value={form.serverName} onChange={(e) => set('serverName', e.target.value)} placeholder="gbase01" />
        </Field>
      )}
      {form.driver === 'sqlserver' && (
        <Field label={t('connections.sqlserver.auth')}>
          <Radio.Group
            value={form.sqlAuth}
            onChange={(e) => set('sqlAuth', e.target.value)}
            options={(['sql', 'windows', 'integrated'] as const).map((value) => ({ value, label: t(`connections.sqlserver.authOption.${value}`) }))}
          />
          {form.sqlAuth !== 'sql' && (
            <div className="muted" style={{ marginTop: 4 }}>
              {t(form.sqlAuth === 'windows' ? 'connections.sqlserver.windowsHint' : 'connections.sqlserver.integratedHint')}
            </div>
          )}
        </Field>
      )}
      {needsCredentials(form) && (
        <Field label={t('connections.general.user')} error={errors.user}>
          <Input
            value={form.user}
            onChange={(e) => set('user', e.target.value)}
            autoComplete="off"
            placeholder={form.driver === 'sqlserver' && form.sqlAuth === 'windows' ? 'HOST\\user' : undefined}
          />
        </Field>
      )}
      {needsCredentials(form) && <Field label={t('connections.general.password')}>
        <Space.Compact style={{ width: '100%' }}>
          <Input.Password
            value={form.password}
            onChange={(e) => set('password', e.target.value)}
            autoComplete="new-password"
            placeholder={editingId ? t('connections.general.passwordKeepPlaceholder') : undefined}
          />
        </Space.Compact>
        <Checkbox checked={form.savePassword} onChange={(e) => set('savePassword', e.target.checked)} style={{ marginTop: 4 }}>
          {t('connections.general.savePassword')}
        </Checkbox>
      </Field>}
      <Field label={databaseLabel(form.driver)} error={errors.database}>
        <Input
          value={form.database}
          onChange={(e) => set('database', e.target.value)}
          placeholder={
            form.driver === 'oracle' ? t('connections.general.oracleServicePlaceholder') : t('connections.general.optional')
          }
        />
      </Field>
      <Field label={t('connections.general.environment')}>
        <Radio.Group
          optionType="button"
          value={form.environment}
          onChange={(e) => set('environment', e.target.value)}
          options={ENVS.map((value) => ({ value, label: t(`connections.env.${value}`) }))}
        />
        {form.environment === 'prod' && (
          <div className="muted" style={{ marginTop: 4 }}>
            {t('connections.general.prodHint')}
          </div>
        )}
      </Field>
    </>
  )

  const tls = supportsTls(form.driver) ? (
    <>
      <Field label={t('connections.tls.mode')}>
        <Radio.Group
          value={form.tls}
          onChange={(e) => set('tls', e.target.value)}
          options={[
            { value: 'disable', label: t('connections.tls.disable') },
            { value: 'require', label: t('connections.tls.require') },
            { value: 'verify', label: t('connections.tls.verify') },
          ]}
        />
      </Field>
      {form.tls === 'verify' && (
        <Field label={t('connections.tls.ca')}>
          <Input value={form.tlsCa} onChange={(e) => set('tlsCa', e.target.value)} placeholder={t('connections.tls.caPlaceholder')} />
        </Field>
      )}
      {form.driver === 'sqlserver' && (
        <Field label={t('connections.sqlserver.trustCert')}>
          <Checkbox checked={form.trustCert} onChange={(e) => set('trustCert', e.target.checked)}>
            {t('connections.sqlserver.trustCertHint')}
          </Checkbox>
        </Field>
      )}
    </>
  ) : (
    <Alert type="info" showIcon message={t('connections.tls.unsupported', { driver: driverLabel(form.driver) })} />
  )

  const advanced = (
    <>
      <Field label={t('connections.advanced.timeout')} error={errors.timeoutSecs}>
        <InputNumber min={1} max={600} value={form.timeoutSecs} onChange={(v) => set('timeoutSecs', v ?? 0)} />
      </Field>
      {form.driver === 'informix' && (
        <Field label="DB_LOCALE">
          <Input value={form.dbLocale} onChange={(e) => set('dbLocale', e.target.value)} placeholder="zh_CN.utf8" />
        </Field>
      )}
      {form.driver === 'sqlserver' && (
        <Field label={t('connections.sqlserver.instance')}>
          <Input value={form.instance} onChange={(e) => set('instance', e.target.value)} placeholder={t('connections.sqlserver.instancePlaceholder')} />
        </Field>
      )}
      {form.driver === 'oracle' && (
        <Field label="SID">
          <Input value={form.oracleSid} onChange={(e) => set('oracleSid', e.target.value)} placeholder={t('connections.advanced.sidPlaceholder')} />
        </Field>
      )}
      <Field label={t('connections.advanced.color')}>
        <Input value={form.color} onChange={(e) => set('color', e.target.value)} placeholder={t('connections.advanced.colorPlaceholder')} />
      </Field>
      <Field label={t('connections.advanced.extra')} error={errors.extra}>
        <Input.TextArea rows={3} value={form.extra} onChange={(e) => set('extra', e.target.value)} placeholder={t('connections.advanced.extraPlaceholder')} />
      </Field>
    </>
  )

  return (
    <Modal
      open={connectionDialog !== null}
      title={editingId ? t('connections.dialog.titleEdit') : t('connections.dialog.titleNew')}
      width={760}
      onCancel={closeConnection}
      destroyOnHidden
      footer={
        <div className="row" style={{ gap: 8 }}>
          <Button icon={<IconPlug size={14} />} loading={busy === 'test'} onClick={test}>
            {t('connections.dialog.test')}
          </Button>
          <span className="grow" />
          <Button onClick={closeConnection}>{t('connections.dialog.cancel')}</Button>
          <Button type="primary" loading={busy === 'save'} onClick={save}>
            {t('connections.dialog.save')}
          </Button>
        </div>
      }
    >
      <div className="row" style={{ alignItems: 'stretch', minHeight: 430 }}>
        <div style={{ width: 170, borderRight: '1px solid var(--border)', paddingRight: 8, marginRight: 16 }}>
          {PRODUCT_GROUPS.map((g) => (
            <div key={g.key}>
              <div className="product-group">{t(`connections.dialog.group.${g.key}`)}</div>
              {g.products.map(product).map((p) => (
                <div
                  key={p.id}
                  className="row"
                  onClick={() => setForm((f) => switchProduct(f, p.id))}
                  style={{
                    gap: 8,
                    padding: '4px 10px',
                    borderRadius: 4,
                    cursor: 'pointer',
                    background: form.product === p.id ? 'var(--sel)' : undefined,
                  }}
                >
                  <DbIcon driver={p.driver} variant={p.id} size={16} />
                  {productLabel(p.id)}
                </div>
              ))}
            </div>
          ))}
          <div className="muted" style={{ marginTop: 12, lineHeight: 1.5 }}>
            {productNote(form.product) || driverNote(form.driver)}
          </div>
        </div>
        <Form className="grow" layout="horizontal" labelCol={{ flex: '100px' }} labelAlign="left">
          <Tabs
            size="small"
            items={[
              { key: 'general', label: t('connections.dialog.tabGeneral'), children: general },
              { key: 'tls', label: 'SSL/TLS', children: tls },
              { key: 'advanced', label: t('connections.dialog.tabAdvanced'), children: advanced },
            ]}
          />
          {result &&
            ('ok' in result ? (
              <Alert
                type="success"
                showIcon
                icon={<IconCircleCheck size={16} />}
                message={t('connections.dialog.testOk', { server: serverText(result.ok), variant: result.ok.variant_name })}
                description={
                  serverNotes(result.ok, form.product).length > 0 &&
                  serverNotes(result.ok, form.product).map((n) => (
                    <div key={n.text} style={n.warn ? { color: 'var(--warn)' } : undefined}>
                      {n.text}
                    </div>
                  ))
                }
              />
            ) : (
              <Alert type="error" showIcon message={result.error} />
            ))}
        </Form>
      </div>
    </Modal>
  )
}
