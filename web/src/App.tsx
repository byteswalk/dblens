// 联调外壳：只用于验证接口和桌面外壳，正式界面在 M1 的设计稿确认后再做（设计文档 7.1 节）。

import { App as AntApp, ConfigProvider, Layout, Result, Tabs, Typography, theme } from 'antd'
import zhCN from 'antd/locale/zh_CN'
import { useMemo } from 'react'

import { createApi } from './api'
import ConnectionPage from './pages/ConnectionPage'
import DriversPage from './pages/DriversPage'
import QueryPage from './pages/QueryPage'
import { useSystemDark } from './theme'

export default function App({ token }: { token: string | null }) {
  const dark = useSystemDark()
  const api = useMemo(() => (token ? createApi(token) : null), [token])
  return (
    <ConfigProvider locale={zhCN} theme={{ algorithm: dark ? theme.darkAlgorithm : theme.defaultAlgorithm }}>
      <AntApp>
        <Layout style={{ minHeight: '100vh' }}>
          <Layout.Content style={{ padding: 16 }}>
            <Typography.Title level={4} style={{ marginTop: 0 }}>
              DBLens 联调页
            </Typography.Title>
            {api ? (
              <Tabs
                items={[
                  { key: 'drivers', label: '驱动', children: <DriversPage api={api} /> },
                  { key: 'connection', label: '连接', children: <ConnectionPage api={api} /> },
                  { key: 'query', label: '查询', children: <QueryPage api={api} /> },
                ]}
              />
            ) : (
              <Result
                status="warning"
                title="缺少访问令牌"
                subTitle="请使用 dblens-server 启动时打印的链接打开本页（链接末尾带 #token=…）。"
              />
            )}
          </Layout.Content>
        </Layout>
      </AntApp>
    </ConfigProvider>
  )
}
