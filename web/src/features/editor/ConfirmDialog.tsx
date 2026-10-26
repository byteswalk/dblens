// 生产确认框（M1 设计文档 1 节）：显示语句和原因，按钮为"取消 / 仍然执行"。不要求输入库名。

import { Modal } from 'antd'
import { useTranslation } from 'react-i18next'

export interface ConfirmDialogProps {
  open: boolean
  /** 需要确认的语句。 */
  sql: string
  reason: string
  onConfirm(): void
  onCancel(): void
}

export default function ConfirmDialog({ open, sql, reason, onConfirm, onCancel }: ConfirmDialogProps) {
  const { t } = useTranslation()
  return (
    <Modal
      open={open}
      title={t('editor.confirm.title')}
      okText={t('editor.confirm.ok')}
      okButtonProps={{ danger: true }}
      cancelText={t('editor.confirm.cancel')}
      onOk={onConfirm}
      onCancel={onCancel}
      width={560}
      destroyOnHidden
    >
      <div style={{ color: 'var(--danger)', margin: '8px 0' }}>{reason}</div>
      <pre className="confirm-sql">{sql}</pre>
    </Modal>
  )
}
