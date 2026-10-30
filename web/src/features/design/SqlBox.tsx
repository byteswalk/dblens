// 只读的 SQL 查看框（与"查看 DDL"相同的 Monaco 设置），设计器的 SQL 预览和保存确认框共用。引入 Monaco，由使用方懒加载。

import Editor from '@monaco-editor/react'

import type { DriverId } from '../../shared/api'
import { useSetting } from '../../shared/settings'
import { THEME_KEY, parseThemePref, resolveTheme, useSystemDark } from '../../shared/theme'
import { editorLanguage } from '../editor/dialect'
import '../editor/monaco'

export default function SqlBox({ sql, driver, height }: { sql: string; driver: DriverId | undefined; height: number | string }) {
  const theme = resolveTheme(parseThemePref(useSetting(THEME_KEY)), useSystemDark())
  return (
    <div style={{ height, border: '1px solid var(--border)' }}>
      <Editor
        language={driver ? editorLanguage(driver) : 'sql'}
        value={sql}
        theme={`dblens-${theme}`}
        options={{
          readOnly: true,
          domReadOnly: true,
          fontFamily: "'JetBrains Mono', 'Cascadia Mono', Consolas, 'Noto Sans Mono CJK SC', monospace",
          fontSize: 14,
          lineHeight: 22,
          minimap: { enabled: false },
          scrollBeyondLastLine: false,
          automaticLayout: true,
          wordWrap: 'on',
        }}
      />
    </div>
  )
}
