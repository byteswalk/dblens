// 提交前的预览（M2a 设计文档 6.4 节）：列出要执行的语句（值写成字面量），生产连接时顶部标红。
// 按钮：取消 / 复制到新查询标签 / 执行。

import { Alert, Button, Modal } from 'antd'
import { useTranslation } from 'react-i18next'

export interface ChangePreviewProps {
  statements: { index: number; sql: string }[] | null
  prod: boolean
  busy: boolean
  onCancel(): void
  onOpenInTab(): void
  onExecute(): void
}

export default function ChangePreview({ statements, prod, busy, onCancel, onOpenInTab, onExecute }: ChangePreviewProps) {
  const { t } = useTranslation()
  return (
    <Modal
      open={statements !== null}
      width={820}
      title={t('editor.data.edit.previewTitle', { count: statements?.length ?? 0 })}
      onCancel={onCancel}
      footer={[
        <Button key="cancel" onClick={onCancel}>
          {t('editor.data.edit.cancel')}
        </Button>,
        <Button key="tab" onClick={onOpenInTab}>
          {t('editor.data.edit.openInTab')}
        </Button>,
        <Button key="run" type="primary" danger={prod} loading={busy} onClick={onExecute}>
          {t('editor.data.edit.execute')}
        </Button>,
      ]}
    >
      {prod && <Alert type="error" showIcon message={t('editor.data.edit.prodWarning')} style={{ marginBottom: 12 }} />}
      <pre className="confirm-sql" style={{ maxHeight: 420 }}>
        {(statements ?? []).map((s) => `${s.sql};`).join('\n')}
      </pre>
    </Modal>
  )
}
