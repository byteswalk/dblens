// 桌面外壳的状态（最大化、置顶、自动检查更新）：启动时取一次，之后跟随 Rust 端推送的事件更新。浏览器模式为 null。

import { useEffect, useState } from 'react'

import { type ShellInfo, desktop } from '../shared/shell'

export function useShellInfo(): ShellInfo | null {
  const [info, setInfo] = useState<ShellInfo | null>(null)
  useEffect(() => {
    if (!desktop) return
    let unlisten: (() => void) | undefined
    let cancelled = false
    desktop.info().then((i) => !cancelled && setInfo(i), console.warn)
    desktop
      .onChange((i) => setInfo(i))
      .then((u) => {
        if (cancelled) u()
        else unlisten = u
      }, console.warn)
    return () => {
      cancelled = true
      unlisten?.()
    }
  }, [])
  return info
}
