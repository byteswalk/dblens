// Monaco 的初始化（M1 设计文档 2.1 节）：从本地打包的 monaco-editor 加载（不走 CDN，离线可用），
// 只用编辑器自身的 worker；注册 PL/SQL 高亮（Oracle、达梦）和跟随界面主题的两套配色。SQL Server 用自带的 sql 语言。
// 只由懒加载的工作区引入，不进入首屏。

import { loader } from '@monaco-editor/react'
import * as monaco from 'monaco-editor'
import EditorWorker from 'monaco-editor/editor/editor.worker?worker'

import { plsqlConf, plsqlLanguage } from './plsql'

self.MonacoEnvironment = { getWorker: () => new EditorWorker() }
loader.config({ monaco })

monaco.languages.register({ id: 'plsql', aliases: ['PL/SQL', 'plsql'] })
monaco.languages.setLanguageConfiguration('plsql', plsqlConf)
monaco.languages.setMonarchTokensProvider('plsql', plsqlLanguage)

// 背景色与 app/shell.css 的 --panel 一致
monaco.editor.defineTheme('dblens-light', { base: 'vs', inherit: true, rules: [], colors: { 'editor.background': '#ffffff' } })
monaco.editor.defineTheme('dblens-dark', {
  base: 'vs-dark',
  inherit: true,
  rules: [],
  colors: { 'editor.background': '#1f1f23' },
})

monaco.editor.defineTheme('dblens-slate', {
  base: 'vs-dark',
  inherit: true,
  rules: [],
  colors: { 'editor.background': '#172029' },
})
monaco.editor.defineTheme('dblens-rose', { base: 'vs', inherit: true, rules: [], colors: { 'editor.background': '#ffffff' } })

export { monaco }
