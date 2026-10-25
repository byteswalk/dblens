import { App, Button, Input, Space, Table, Tag } from 'antd'
import { useCallback, useEffect, useState } from 'react'

import type { Api, DriverStatus } from '../api'

/** 驱动状态；Oracle、达梦要先确认原生客户端库目录才能连接。 */
export default function DriversPage({ api }: { api: Api }) {
  const { message } = App.useApp()
  const [drivers, setDrivers] = useState<DriverStatus[]>([])
  const [dirs, setDirs] = useState<Record<string, string>>({})

  const load = useCallback(() => {
    api.drivers().then(setDrivers, (e: Error) => message.error(e.message))
  }, [api, message])
  useEffect(load, [load])

  const confirm = async (d: DriverStatus) => {
    const path = (dirs[d.id] ?? d.lib_dir ?? d.suggested_lib_dir ?? '').trim()
    try {
      await api.setLibDir(d.id, path || null)
      load()
    } catch (e) {
      message.error((e as Error).message)
    }
  }

  return (
    <Table<DriverStatus>
      rowKey="id"
      pagination={false}
      dataSource={drivers}
      columns={[
        { title: '驱动', dataIndex: 'name' },
        {
          title: '状态',
          render: (_, d) => (d.available ? <Tag color="green">可用</Tag> : <Tag color="orange">不可用</Tag>),
        },
        {
          title: '原生库目录',
          render: (_, d) =>
            d.needs_native_lib ? (
              <Space.Compact style={{ width: '100%' }}>
                <Input
                  placeholder={d.suggested_lib_dir ?? '绝对路径'}
                  value={dirs[d.id] ?? d.lib_dir ?? d.suggested_lib_dir ?? ''}
                  onChange={(e) => setDirs({ ...dirs, [d.id]: e.target.value })}
                />
                <Button onClick={() => confirm(d)}>确认</Button>
              </Space.Compact>
            ) : (
              '不需要'
            ),
        },
        { title: '提示', dataIndex: 'hint' },
      ]}
    />
  )
}
