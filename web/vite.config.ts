/// <reference types="vitest/config" />
import react from '@vitejs/plugin-react'
import { defineConfig } from 'vite'

// 开发时把 /api（含 WebSocket）转发给本机的 dblens-server：
//   dblens-server --allow-origin http://localhost:5173
// 再打开 dblens-server 打印的链接，把其中的 http://127.0.0.1:8742 换成 http://localhost:5173。
export default defineConfig({
  plugins: [react()],
  server: {
    port: 5173,
    strictPort: true,
    proxy: {
      '/api': { target: 'http://127.0.0.1:8742', ws: true },
    },
  },
  build: {
    outDir: 'dist',
    chunkSizeWarningLimit: 2048,
  },
  // SQL 语言 worker 按方言懒加载解析器：需要 ES 模块格式才能拆块（默认的 iife 会把所有解析器打进一个文件）
  worker: { format: 'es' },
  test: {
    setupFiles: ['./src/test-setup.ts'],
  },
})
