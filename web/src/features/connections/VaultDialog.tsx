// 设置主密码（第一次保存密码时）或输入主密码解锁（每次启动后第一次用到密码时）。

import { Alert, Form, Input, Modal } from 'antd'
import { useEffect, useState } from 'react'

import { useApi } from '../../app/context'
import { useVault } from './vault'

export default function VaultDialog() {
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
    if (!master) return setError('输入主密码')
    if (init && master !== again) return setError('两次输入的主密码不一致')
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
      title={init ? '设置主密码' : '解锁密码库'}
      okText={init ? '设置' : '解锁'}
      confirmLoading={busy}
      onOk={submit}
      onCancel={() => finish(false)}
      width={420}
    >
      <p style={{ color: 'var(--text-2)' }}>
        {init
          ? '这台电脑没有可用的系统钥匙串，连接密码会用主密码加密后保存在本地。主密码遗忘后无法找回，只能重新输入各连接的密码。'
          : '连接密码保存在本地的加密密码库中，输入主密码解锁后才能使用。'}
      </p>
      <Form layout="vertical" onFinish={submit}>
        <Form.Item label="主密码">
          <Input.Password autoFocus value={master} onChange={(e) => setMaster(e.target.value)} onPressEnter={submit} />
        </Form.Item>
        {init && (
          <Form.Item label="再输入一次">
            <Input.Password value={again} onChange={(e) => setAgain(e.target.value)} onPressEnter={submit} />
          </Form.Item>
        )}
      </Form>
      {error && <Alert type="error" showIcon message={error} />}
    </Modal>
  )
}
