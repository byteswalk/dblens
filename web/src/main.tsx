import { StrictMode } from 'react'
import { createRoot } from 'react-dom/client'

import App from './App'
import { TOKEN_IN_HASH, bootstrapToken } from './token'

const token = bootstrapToken(window as unknown as Parameters<typeof bootstrapToken>[0])

// 页面已打开时再打开新的链接（如 dblens-server 重启后的新令牌），只有 # 后面变了，浏览器不会重新加载页面
window.addEventListener('hashchange', () => {
  if (TOKEN_IN_HASH.test(window.location.hash)) window.location.reload()
})

createRoot(document.getElementById('root')!).render(
  <StrictMode>
    <App token={token} />
  </StrictMode>,
)
