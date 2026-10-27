// 各语言的词条，按功能模块分文件；键为 "<模块>.<…>"，如 editor.toolbar.runCurrent。
// 中文是键的来源（见 shared/i18next.d.ts），英文必须有同样的键（locales.test.ts 检查）。

import enApp from './en/app.json'
import enCommon from './en/common.json'
import enConnections from './en/connections.json'
import enEditor from './en/editor.json'
import enImport from './en/import.json'
import enResults from './en/results.json'
import enTree from './en/tree.json'
import zhApp from './zh-CN/app.json'
import zhCommon from './zh-CN/common.json'
import zhConnections from './zh-CN/connections.json'
import zhEditor from './zh-CN/editor.json'
import zhImport from './zh-CN/import.json'
import zhResults from './zh-CN/results.json'
import zhTree from './zh-CN/tree.json'

export const zhCN = {
  common: zhCommon,
  app: zhApp,
  connections: zhConnections,
  tree: zhTree,
  editor: zhEditor,
  results: zhResults,
  import: zhImport,
}

export const en = {
  common: enCommon,
  app: enApp,
  connections: enConnections,
  tree: enTree,
  editor: enEditor,
  results: enResults,
  import: enImport,
}
