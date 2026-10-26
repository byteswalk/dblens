// 设置主密码（第一次保存密码时）或输入主密码解锁（每次启动后第一次用到密码时）。

import { Alert, Form, Input, Modal } from 'antd'
import { useEffect, useState } from 'react'
import { useTranslation } from 'react-i18next'

import { useApi } from '../../app/context'
import { useVault } from './vault'

export default function VaultDialog() {
  const { t } = useTranslation()
  const api = useApi()
  const { pending, finish } = useVault()
  const [master, setMaster] = useState('')
  const [again, setAgain] = useState('')
  const [error, setError] = useState<string | null>(null)
  const [busy, setBusy] = useState(false)

  useEffect(() => {
    setMaster('')
    setAgain('')
    setError(null)
  }, [pending])

  if (!pending) return null
  const init = pending.mode === 'init'

  const submit = async () => {
    if (!master) return setError(t('connections.vault.masterRequired'))
    if (init && master !== again) return setError(t('connections.vault.mismatch'))
    setBusy(true)
    try {
      await (init ? api.initVault(master) : api.unlockVault(master))
      finish(true)
    } catch (e) {
      setError((e as Error).message)
    } finally {
      setBusy(false)
    }
  }

  return (
    <Modal
      open
      title={init ? t('connections.vault.titleInit') : t('connections.vault.titleUnlock')}
      okText={init ? t('connections.vault.okInit') : t('connections.vault.okUnlock')}
      confirmLoading={busy}
      onOk={submit}
      onCancel={() => finish(false)}
      width={420}
    >
      <p style={{ color: 'var(--text-2)' }}>
        {init ? t('connections.vault.introInit') : t('connections.vault.introUnlock')}
      </p>
      <Form layout="vertical" onFinish={submit}>
        <Form.Item label={t('connections.vault.master')}>
          <Input.Password autoFocus value={master} onChange={(e) => setMaster(e.target.value)} onPressEnter={submit} />
        </Form.Item>
        {init && (
          <Form.Item label={t('connections.vault.again')}>
            <Input.Password value={again} onChange={(e) => setAgain(e.target.value)} onPressEnter={submit} />
          </Form.Item>
        )}
      </Form>
      {error && <Alert type="error" showIcon message={error} />}
    </Modal>
  )
}
