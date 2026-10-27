// 查看 DDL（M1c2 设计文档第 3 节）：只读的 Monaco 预览，按连接的语言高亮；可复制、在新标签中打开。
// 引入 Monaco，由对象树懒加载（见 ObjectTree 的 DdlDialog）。

import Editor from '@monaco-editor/react'
import { useQuery } from '@tanstack/react-query'
import { App, Button, Modal, Spin } from 'antd'
import { useTranslation } from 'react-i18next'

import { useApi } from '../../app/context'
import type { DdlKind, DriverId } from '../../shared/api'
import { useSetting } from '../../shared/settings'
import { THEME_KEY, parseThemePref, resolveTheme, useSystemDark } from '../../shared/theme'
import { useQueryActions } from '../editor/actions'
import { editorLanguage } from '../editor/dialect'
import '../editor/monaco'
import { withVault } from '../connections/vault'

export interface DdlTarget {
  profileId: string
  driver: DriverId
  schema: string
  kind: DdlKind
  name: string
}

export default function DdlDialog({ target, onClose }: { target: DdlTarget; onClose(): void }) {
  const api = useApi()
  const { t } = useTranslation()
  const { message } = App.useApp()
  const { openSql } = useQueryActions()
  const theme = resolveTheme(parseThemePref(useSetting(THEME_KEY)), useSystemDark())
  const { profileId, schema, kind, name } = target
  // 每次打开都重新取（缓存在后端），不在前端缓存
  const { data, error, isFetching } = useQuery({
    queryKey: ['ddl', profileId, schema, kind, name],
    queryFn: () => withVault(api, () => api.ddl(profileId, schema, kind, name)),
    gcTime: 0,
    retry: false,
  })

  return (
    <Modal
      open
      width={820}
      title={t('tree.ddl.title', { name: `${schema}.${name}` })}
      onCancel={onClose}
      footer={[
        <Button
          key="copy"
          disabled={!data}
          onClick={() =>
            void navigator.clipboard.writeText(data ?? '').then(() => message.success(t('tree.copied')))
          }
        >
          {t('tree.ddl.copy')}
        </Button>,
        <Button
          key="open"
          disabled={!data}
          onClick={() => {
            openSql(data ?? '', profileId)
            onClose()
          }}
        >
          {t('tree.ddl.openInTab')}
        </Button>,
        <Button key="close" type="primary" onClick={onClose}>
          {t('tree.ddl.close')}
        </Button>,
      ]}
    >
      <div style={{ height: 460, border: '1px solid var(--border)' }}>
        {isFetching ? (
          <Spin style={{ margin: 48 }} />
        ) : error ? (
          <div className="err" style={{ padding: 12, whiteSpace: 'pre-wrap' }}>
            {(error as Error).message}
          </div>
        ) : (
          <Editor
            language={editorLanguage(target.driver)}
            value={data ?? ''}
            theme={theme === 'dark' ? 'dblens-dark' : 'dblens-light'}
            options={{
              readOnly: true,
              domReadOnly: true,
              fontFamily: "'JetBrains Mono', 'Cascadia Mono', Consolas, 'Noto Sans Mono CJK SC', monospace",
              fontSize: 14,
              lineHeight: 22,
              minimap: { enabled: false },
              scrollBeyondLastLine: false,
              automaticLayout: true,
            }}
          />
        )}
      </div>
    </Modal>
  )
}
