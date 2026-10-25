// 主题跟随系统（设计文档 7.2 节）：系统切换亮色/暗色时实时生效。
// 固定为亮色或暗色的设置项在 M1 的设置页中实现。

import { useSyncExternalStore } from 'react'

const QUERY = '(prefers-color-scheme: dark)'

function subscribe(onChange: () => void) {
  const mq = window.matchMedia(QUERY)
  mq.addEventListener('change', onChange)
  return () => mq.removeEventListener('change', onChange)
}

export function useSystemDark(): boolean {
  return useSyncExternalStore(subscribe, () => window.matchMedia(QUERY).matches)
}
