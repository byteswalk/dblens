// 主题（设计文档 7.2 节；M1 设计文档 2 节）："跟随系统 / 亮色 / 暗色"三选一，跟随系统时随系统主题实时切换。
// 选定的主题写到 <html data-theme>，供 app/shell.css 中的 CSS 变量使用；antd 的算法由 App 按同一结果设置。

import { useSyncExternalStore } from 'react'

export type ThemePref = 'system' | 'light' | 'dark'
export type Theme = 'light' | 'dark'

export const THEME_KEY = 'ui.theme'

export function parseThemePref(v: string | undefined): ThemePref {
  return v === 'light' || v === 'dark' ? v : 'system'
}

export function resolveTheme(pref: ThemePref, systemDark: boolean): Theme {
  if (pref === 'system') return systemDark ? 'dark' : 'light'
  return pref
}

const QUERY = '(prefers-color-scheme: dark)'

function subscribe(onChange: () => void) {
  const mq = window.matchMedia(QUERY)
  mq.addEventListener('change', onChange)
  return () => mq.removeEventListener('change', onChange)
}

export function useSystemDark(): boolean {
  return useSyncExternalStore(subscribe, () => window.matchMedia(QUERY).matches)
}
