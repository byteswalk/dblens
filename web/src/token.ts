// 访问令牌的来源（设计文档 4.1 节）：
// - 桌面模式：Tauri 在页面脚本运行前注入 window.__DBLENS__.token；
// - 浏览器模式：dblens-server 打印的链接带 #token=…。取出后存进 sessionStorage（刷新页面仍可用），
//   并从地址栏去掉，避免被截图或复制出去。

export const TOKEN_KEY = 'dblens.token'

/** 只用到 window 的这几个部分，测试时可以传入假对象。 */
export interface TokenHost {
  __DBLENS__?: { token?: string }
  location: { hash: string; pathname: string; search: string }
  history: { replaceState(data: unknown, unused: string, url: string): void }
  sessionStorage: { getItem(key: string): string | null; setItem(key: string, value: string): void }
}

export function bootstrapToken(host: TokenHost): string | null {
  const injected = host.__DBLENS__?.token
  if (injected) return injected
  const m = /^#(?:.*&)?token=([^&]+)/.exec(host.location.hash)
  if (m) {
    const token = decodeURIComponent(m[1])
    try {
      host.sessionStorage.setItem(TOKEN_KEY, token)
    } catch {
      // 存储不可用（隐私模式等）时只在本页有效
    }
    host.history.replaceState(null, '', host.location.pathname + host.location.search)
    return token
  }
  try {
    return host.sessionStorage.getItem(TOKEN_KEY)
  } catch {
    return null
  }
}
