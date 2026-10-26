import { App, Button, Descriptions, Form, Input, InputNumber, Popconfirm, Select, Space, Table } from 'antd'
import { useCallback, useEffect, useState } from 'react'

import type { Api, DriverId, Profile, ProfileInput, ServerSummary } from '../shared/api'

const DEFAULT_PORTS: Record<DriverId, number> = { mysql: 3306, postgres: 5432, oracle: 1521, dameng: 5236 }

interface FormValues {
  name: string
  driver: DriverId
  host: string
  port: number
  user: string
  password: string
  database?: string
  options?: string
}

/** 表单里的选项按 `键=值`，一行一个。 */
function toInput(v: FormValues): ProfileInput {
  const options: Record<string, string> = {}
  for (const line of (v.options ?? '').split('\n')) {
    const i = line.indexOf('=')
    if (i > 0) options[line.slice(0, i).trim()] = line.slice(i + 1).trim()
  }
  return {
    name: v.name,
    driver: v.driver,
    host: v.host,
    port: v.port,
    user: v.user,
    database: v.database || null,
    options,
  }
}

/** 测试连接、保存连接配置。 */
export default function ConnectionPage({ api }: { api: Api }) {
  const { message } = App.useApp()
  const [form] = Form.useForm<FormValues>()
  const [summary, setSummary] = useState<ServerSummary | null>(null)
  const [busy, setBusy] = useState(false)
  const [profiles, setProfiles] = useState<Profile[]>([])

  const load = useCallback(() => {
    api.profiles().then(setProfiles, (e: Error) => message.error(e.message))
  }, [api, message])
  useEffect(load, [load])

  const run = async (f: (v: FormValues) => Promise<void>) => {
    const v = await form.validateFields()
    setBusy(true)
    try {
      await f(v)
    } catch (e) {
      message.error((e as Error).message)
    } finally {
      setBusy(false)
    }
  }

  const test = () =>
    run(async (v) => {
      setSummary(null)
      setSummary(await api.testConnection(toInput(v), v.password ?? ''))
    })

  const save = () =>
    run(async (v) => {
      await api.createProfile(toInput(v), v.password || null)
      message.success('已保存')
      load()
    })

  return (
    <Space direction="vertical" size="large" style={{ width: '100%' }}>
      <Form<FormValues>
        form={form}
        layout="horizontal"
        labelCol={{ span: 4 }}
        wrapperCol={{ span: 12 }}
        initialValues={{ driver: 'mysql', port: 3306, host: '127.0.0.1' }}
        onValuesChange={(changed: Partial<FormValues>) => {
          if (changed.driver) form.setFieldValue('port', DEFAULT_PORTS[changed.driver])
        }}
      >
        <Form.Item name="name" label="名称" rules={[{ required: true }]}>
          <Input />
        </Form.Item>
        <Form.Item name="driver" label="驱动">
          <Select
            options={[
              { value: 'mysql', label: 'MySQL / MariaDB / TiDB / OceanBase' },
              { value: 'postgres', label: 'PostgreSQL / 金仓 / openGauss' },
              { value: 'oracle', label: 'Oracle' },
              { value: 'dameng', label: '达梦' },
            ]}
          />
        </Form.Item>
        <Form.Item name="host" label="主机" rules={[{ required: true }]}>
          <Input />
        </Form.Item>
        <Form.Item name="port" label="端口" rules={[{ required: true }]}>
          <InputNumber min={1} max={65535} />
        </Form.Item>
        <Form.Item name="user" label="用户" rules={[{ required: true }]}>
          <Input />
        </Form.Item>
        <Form.Item name="password" label="密码">
          <Input.Password autoComplete="new-password" />
        </Form.Item>
        <Form.Item name="database" label="库 / 服务名">
          <Input />
        </Form.Item>
        <Form.Item name="options" label="选项" extra="一行一个，如 tls=require">
          <Input.TextArea rows={2} />
        </Form.Item>
        <Form.Item wrapperCol={{ offset: 4 }}>
          <Space>
            <Button type="primary" loading={busy} onClick={test}>
              测试连接
            </Button>
            <Button loading={busy} onClick={save}>
              保存
            </Button>
          </Space>
        </Form.Item>
      </Form>
      {summary && (
        <Descriptions bordered size="small" column={1} title="连接成功">
          <Descriptions.Item label="产品">{summary.product}</Descriptions.Item>
          <Descriptions.Item label="版本">{summary.version}</Descriptions.Item>
          <Descriptions.Item label="识别为">{summary.variant_name}</Descriptions.Item>
        </Descriptions>
      )}
      <Table<Profile>
        rowKey="id"
        size="small"
        pagination={false}
        dataSource={profiles}
        columns={[
          { title: '名称', dataIndex: 'name' },
          { title: '驱动', dataIndex: 'driver' },
          { title: '地址', render: (_, p) => `${p.user}@${p.host}:${p.port}` },
          {
            title: '',
            render: (_, p) => (
              <Popconfirm
                title="删除这个连接配置？"
                onConfirm={() => api.deleteProfile(p.id).then(load, (e: Error) => message.error(e.message))}
              >
                <Button size="small" danger>
                  删除
                </Button>
              </Popconfirm>
            ),
          },
        ]}
      />
    </Space>
  )
}
