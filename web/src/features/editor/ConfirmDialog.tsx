// 生产确认框（M1 设计文档 1 节）：显示语句和原因，按钮为"取消 / 仍然执行"。不要求输入库名。

import { Modal } from 'antd'

export interface ConfirmDialogProps {
  open: boolean
  /** 需要确认的语句。 */
  sql: string
  reason: string
  onConfirm(): void
  onCancel(): void
}

export default function ConfirmDialog({ open, sql, reason, onConfirm, onCancel }: ConfirmDialogProps) {
  return (
    <Modal
      open={open}
      title="确认执行"
      okText="仍然执行"
      okButtonProps={{ danger: true }}
      cancelText="取消"
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
