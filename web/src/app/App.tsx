// 应用入口：拿到令牌后读取设置，按主题配置 antd，挂上外壳和全局对话框。

import { App as AntApp, ConfigProvider, Result, Spin, theme as antdTheme } from 'antd'
import enUS from 'antd/locale/en_US'
import zhCN from 'antd/locale/zh_CN'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { useEffect, useMemo, useState } from 'react'
import { useTranslation } from 'react-i18next'

import ConnectionDialog from '../features/connections/ConnectionDialog'
import { TABS_KEY, persistTabs, useTabs } from '../features/editor/tabs'
import VaultDialog from '../features/connections/VaultDialog'
import { type Api, createApi } from '../shared/api'
import { LANG_KEY, applyLang, parseLangPref, resolveLang } from '../shared/i18n'
import { useSetting, useSettings } from '../shared/settings'
import { THEME_KEY, parseThemePref, resolveTheme, useSystemDark, isDarkTheme, type Theme } from '../shared/theme'
import SettingsDialog from './SettingsDialog'
import Shell from './Shell'
import { installCrashLog } from '../shared/crash'
import CrashBoundary from './CrashBoundary'
import { ApiContext } from './context'
import './shell.css'
import { SIDE_KEY, parseSide, useUi } from './ui'

/** 有自己强调色的主题。 */
const THEME_PRIMARY: Partial<Record<Theme, { colorPrimary: string }>> = {
  slate: { colorPrimary: '#62a9cc' },
  rose: { colorPrimary: '#a84a70' },
}

function Themed() {
  const systemDark = useSystemDark()
  const theme = resolveTheme(parseThemePref(useSetting(THEME_KEY)), systemDark)
  // 在渲染时写，不放进 effect：子组件（结果表格）渲染时要从 CSS 变量读到新主题的颜色
  document.documentElement.dataset.theme = theme
  document.documentElement.dataset.tone = isDarkTheme(theme) ? 'dark' : 'light'
  const lang = resolveLang(parseLangPref(useSetting(LANG_KEY)))
  useEffect(() => applyLang(lang), [lang])
  const algorithm = isDarkTheme(theme) ? antdTheme.darkAlgorithm : antdTheme.defaultAlgorithm
  // 不用 compactAlgorithm：它把字号再减 2px。正文 14px 与 shell.css 的 --fs 一致，控件高度略收紧
  const token = { fontSize: 14, borderRadius: 4, controlHeight: 30, controlHeightSM: 26, ...THEME_PRIMARY[theme] }
  return (
    <ConfigProvider locale={lang === 'en' ? enUS : zhCN} theme={{ algorithm, token }}>
      <AntApp style={{ height: '100%' }}>
        <CrashBoundary>
          <Shell />
        </CrashBoundary>
        <ConnectionDialog />
        <VaultDialog />
        <SettingsDialog />
      </AntApp>
    </ConfigProvider>
  )
}

/** 读取设置后再显示界面，避免主题和布局先按默认值闪一下。 */
function Boot({ api }: { api: Api }) {
  const { t } = useTranslation()
  const [state, setState] = useState<'loading' | 'ready' | Error>('loading')
  useEffect(() => {
    api.settings().then(
      (values) => {
        useSettings.getState().init(values, api.putSetting)
        // 先切好语言再显示界面，避免先按默认语言闪一下
        applyLang(resolveLang(parseLangPref(values[LANG_KEY])))
        useUi.setState({ side: parseSide(values[SIDE_KEY]) })
        useTabs.getState().restore(values[TABS_KEY])
        persistTabs()
        setState('ready')
      },
      (e: Error) => setState(e),
    )
  }, [api])
  if (state === 'loading') return <Spin fullscreen />
  if (state instanceof Error) return <Result status="error" title={t('app.boot.serverUnreachable')} subTitle={state.message} />
  return <Themed />
}

export default function App({ token }: { token: string | null }) {
  const { t } = useTranslation()
  const api = useMemo(() => (token ? createApi(token) : null), [token])
  useEffect(() => {
    if (api) installCrashLog(api.clientLog)
  }, [api])
  const queryClient = useMemo(() => new QueryClient({ defaultOptions: { queries: { retry: false } } }), [])
  if (!api) {
    return (
      <Result
        status="warning"
        title={t('app.boot.missingToken')}
        subTitle={t('app.boot.missingTokenHint')}
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
