// 应用入口：拿到令牌后读取设置，按主题配置 antd，挂上外壳和全局对话框。

import { App as AntApp, ConfigProvider, Result, Spin, theme as antdTheme } from 'antd'
import zhCN from 'antd/locale/zh_CN'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { useEffect, useMemo, useState } from 'react'

import ConnectionDialog from '../features/connections/ConnectionDialog'
import { TABS_KEY, persistTabs, useTabs } from '../features/editor/tabs'
import VaultDialog from '../features/connections/VaultDialog'
import { type Api, createApi } from '../shared/api'
import { useSetting, useSettings } from '../shared/settings'
import { THEME_KEY, parseThemePref, resolveTheme, useSystemDark } from '../shared/theme'
import SettingsDialog from './SettingsDialog'
import Shell from './Shell'
import { ApiContext } from './context'
import './shell.css'
import { SIDE_KEY, parseSide, useUi } from './ui'

function Themed() {
  const systemDark = useSystemDark()
  const theme = resolveTheme(parseThemePref(useSetting(THEME_KEY)), systemDark)
  useEffect(() => {
    document.documentElement.dataset.theme = theme
  }, [theme])
  const algorithm = [theme === 'dark' ? antdTheme.darkAlgorithm : antdTheme.defaultAlgorithm, antdTheme.compactAlgorithm]
  return (
    <ConfigProvider locale={zhCN} theme={{ algorithm, token: { fontSize: 12, borderRadius: 4 } }}>
      <AntApp style={{ height: '100%' }}>
        <Shell />
        <ConnectionDialog />
        <VaultDialog />
        <SettingsDialog />
      </AntApp>
    </ConfigProvider>
  )
}

/** 读取设置后再显示界面，避免主题和布局先按默认值闪一下。 */
function Boot({ api }: { api: Api }) {
  const [state, setState] = useState<'loading' | 'ready' | Error>('loading')
  useEffect(() => {
    api.settings().then(
      (values) => {
        useSettings.getState().init(values, api.putSetting)
        useUi.setState({ side: parseSide(values[SIDE_KEY]) })
        useTabs.getState().restore(values[TABS_KEY])
        persistTabs()
        setState('ready')
      },
      (e: Error) => setState(e),
    )
  }, [api])
  if (state === 'loading') return <Spin fullscreen />
  if (state instanceof Error) return <Result status="error" title="无法连接 DBLens 服务" subTitle={state.message} />
  return <Themed />
}

export default function App({ token }: { token: string | null }) {
  const api = useMemo(() => (token ? createApi(token) : null), [token])
  const queryClient = useMemo(() => new QueryClient({ defaultOptions: { queries: { retry: false } } }), [])
  if (!api) {
    return (
      <Result
        status="warning"
        title="缺少访问令牌"
        subTitle="请使用 dblens-server 启动时打印的链接打开本页（链接末尾带 #token=…）。"
      />
    )
  }
  return (
    <ApiContext.Provider value={api}>
      <QueryClientProvider client={queryClient}>
        <Boot api={api} />
      </QueryClientProvider>
    </ApiContext.Provider>
  )
}
