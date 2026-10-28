// 打开模型、新建模型、从库反向生成（M4a 设计文档 4.2 节）。模型目录是服务所在机器上的路径：
// 桌面版可以用系统的"选择文件夹"对话框，浏览器模式直接填写。反向生成作为任务执行，显示进度、可以取消。

import { IconFolder } from '@tabler/icons-react'
import { useQuery, useQueryClient } from '@tanstack/react-query'
import { App, Button, Form, Input, Modal, Progress, Select, Space } from 'antd'
import { useEffect, useState } from 'react'
import { useTranslation } from 'react-i18next'

import { useApi } from '../../app/context'
import { useUi } from '../../app/ui'
import { type Compat, type TaskStatus, errorText } from '../../shared/api'
import { desktop } from '../../shared/shell'
import { withVault } from '../connections/vault'
import { PRODUCTS, type ProductId, productLabel } from '../connections/products'
import { baseName, useModelDialogs } from './dialogs'

function PathField({ value, onChange }: { value?: string; onChange?(v: string): void }) {
  const { t } = useTranslation()
  return (
    <Space.Compact style={{ width: '100%' }}>
      <Input value={value} placeholder={t('model.dialog.pathHint')} onChange={(e) => onChange?.(e.target.value)} />
      {desktop && (
        <Button
          icon={<IconFolder size={16} />}
          onClick={async () => {
            const p = await desktop?.folderDialog()
            if (p) onChange?.(p)
          }}
        />
      )}
    </Space.Compact>
  )
}

/** 打开后切到模型面板。 */
function useOpened() {
  const qc = useQueryClient()
  return () => {
    void qc.invalidateQueries({ queryKey: ['models'] })
    useUi.getState().setSide('models')
  }
}

function OpenDialog() {
  const api = useApi()
  const { t } = useTranslation()
  const { message } = App.useApp()
  const close = useModelDialogs((s) => s.close)
  const opened = useOpened()
  const [path, setPath] = useState('')
  const [busy, setBusy] = useState(false)
  const submit = async () => {
    setBusy(true)
    try {
      await api.openModel(path.trim())
      opened()
      close()
    } catch (e) {
      message.error((e as Error).message)
    } finally {
      setBusy(false)
    }
  }
  return (
    <Modal open title={t('model.dialog.openTitle')} onCancel={close} onOk={() => void submit()} okButtonProps={{ disabled: !path.trim(), loading: busy }}>
      <Form layout="vertical">
        <Form.Item label={t('model.dialog.path')}>
          <PathField value={path} onChange={setPath} />
        </Form.Item>
      </Form>
    </Modal>
  )
}

/** 新建模型时可选的产品：有兼容模式的（人大金仓、openGauss）另选模式。 */
const COMPAT_PRODUCTS: ProductId[] = ['kingbase', 'opengauss']

function CreateDialog() {
  const api = useApi()
  const { t } = useTranslation()
  const { message } = App.useApp()
  const close = useModelDialogs((s) => s.close)
  const opened = useOpened()
  const [path, setPath] = useState('')
  const [name, setName] = useState('')
  const [product, setProduct] = useState<ProductId>('mysql')
  const [compat, setCompat] = useState<Compat>('native')
  const [busy, setBusy] = useState(false)
  const submit = async () => {
    setBusy(true)
    try {
      const withCompat = COMPAT_PRODUCTS.includes(product)
      await api.createModel(path.trim(), name.trim() || baseName(path), { variant: product, compat: withCompat ? compat : 'native' })
      opened()
      close()
    } catch (e) {
      message.error((e as Error).message)
    } finally {
      setBusy(false)
    }
  }
  return (
    <Modal open title={t('model.dialog.createTitle')} onCancel={close} onOk={() => void submit()} okButtonProps={{ disabled: !path.trim(), loading: busy }}>
      <Form layout="vertical">
        <Form.Item label={t('model.dialog.path')} extra={t('model.dialog.emptyDir')}>
          <PathField value={path} onChange={setPath} />
        </Form.Item>
        <Form.Item label={t('model.dialog.name')}>
          <Input value={name} placeholder={baseName(path)} onChange={(e) => setName(e.target.value)} />
        </Form.Item>
        <Form.Item label={t('model.dialog.product')} extra={t('model.dialog.versionNote')}>
          <Select value={product} onChange={setProduct} options={PRODUCTS.map((p) => ({ value: p.id, label: productLabel(p.id) }))} />
        </Form.Item>
        {COMPAT_PRODUCTS.includes(product) && (
          <Form.Item label={t('model.dialog.compat')}>
            <Select
              value={compat}
              onChange={setCompat}
              options={[
                { value: 'native', label: t('model.dialog.compatNative') },
                { value: 'oracle', label: t('model.dialog.compatOracle') },
              ]}
            />
          </Form.Item>
        )}
      </Form>
    </Modal>
  )
}

function ReverseDialog() {
  const api = useApi()
  const { t } = useTranslation()
  const { message } = App.useApp()
  const init = useModelDialogs((s) => s.reverse)!
  const close = useModelDialogs((s) => s.close)
  const opened = useOpened()
  const { data: profiles = [] } = useQuery({ queryKey: ['profiles'], queryFn: () => api.profiles() })
  const [profileId, setProfileId] = useState(init.profileId ?? useUi.getState().selectedProfile ?? profiles[0]?.id ?? null)
  const [schemas, setSchemas] = useState<string[]>(init.schema ? [init.schema] : [])
  const [path, setPath] = useState('')
  const [name, setName] = useState('')
  const [task, setTask] = useState<TaskStatus | null>(null)
  // 连接列表加载完之前打开的对话框：加载后选第一个
  useEffect(() => {
    if (!profileId && profiles.length) setProfileId(profiles[0].id)
  }, [profileId, profiles])
  const { data: all = [] } = useQuery({
    queryKey: ['schemas', profileId],
    queryFn: () => withVault(api, () => api.schemas(profileId!)),
    enabled: !!profileId,
  })

  // 执行中：轮询任务状态；完成后打开模型
  useEffect(() => {
    if (!task) return
    if (task.state === 'done') {
      message.success(t('model.dialog.reverseDone'))
      opened()
      close()
      return
    }
    if (task.state !== 'running') return
    const h = setTimeout(async () => {
      try {
        setTask(await api.task(task.id))
      } catch (e) {
        message.error((e as Error).message)
        setTask(null)
      }
    }, 400)
    return () => clearTimeout(h)
  }, [task, api, message, t, opened, close])

  const start = async () => {
    try {
      const id = await withVault(api, () =>
        api.startReverse({ profile_id: profileId!, schemas, path: path.trim(), name: name.trim() || baseName(path) }),
      )
      setTask({ id, state: 'running', progress: { read: 0, total: schemas.length } } as TaskStatus)
    } catch (e) {
      message.error((e as Error).message)
    }
  }
  const running = task?.state === 'running'
  return (
    <Modal
      open
      title={t('model.dialog.reverseTitle')}
      onCancel={() => {
        if (running) void api.cancelTask(task!.id).catch(() => {})
        close()
      }}
      onOk={() => void start()}
      okText={t('model.dialog.reverseOk')}
      okButtonProps={{ disabled: !profileId || !schemas.length || !path.trim() || running, loading: running }}
    >
      <Form layout="vertical">
        <Form.Item label={t('model.dialog.profile')}>
          <Select
            value={profileId ?? undefined}
            onChange={(v) => {
              setProfileId(v)
              setSchemas([])
            }}
            options={profiles.map((p) => ({ value: p.id, label: p.name }))}
          />
        </Form.Item>
        <Form.Item label={t('model.dialog.schemas')}>
          <Select mode="multiple" value={schemas} onChange={setSchemas} options={all.map((s) => ({ value: s.name }))} />
        </Form.Item>
        <Form.Item label={t('model.dialog.path')} extra={t('model.dialog.emptyDir')}>
          <PathField value={path} onChange={setPath} />
        </Form.Item>
        <Form.Item label={t('model.dialog.name')}>
          <Input value={name} placeholder={baseName(path)} onChange={(e) => setName(e.target.value)} />
        </Form.Item>
      </Form>
      {task && task.state !== 'running' && task.state !== 'done' && (
        <div className="err">{task.error ? errorText(task.error) : t('model.dialog.reverseFailed')}</div>
      )}
      {running && <Progress size="small" percent={Math.round((100 * task.progress.read) / Math.max(1, task.progress.total))} />}
    </Modal>
  )
}

export default function ModelDialogs() {
  const { open, create, reverse } = useModelDialogs()
  return (
    <>
      {open && <OpenDialog />}
      {create && <CreateDialog />}
      {reverse && <ReverseDialog />}
    </>
  )
}
