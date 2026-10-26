// 单元格查看对话框（设计稿第 3 部分）：文本 / JSON / 十六进制，可复制。

import { App, Button, Modal, Tabs } from 'antd'
import { IconCopy } from '@tabler/icons-react'
import { useState } from 'react'
import { useTranslation } from 'react-i18next'

import type { CellValue } from '../../shared/exec'
import { cellBytes, cellFullText, hexDump, lobNote, prettyJson } from './cell'

export interface ViewedCell {
  column: string
  /** 从 1 开始的行号。 */
  row: number
  value: CellValue
}

export default function CellViewer({ cell, onClose }: { cell: ViewedCell | null; onClose(): void }) {
  const { t } = useTranslation()
  const { message } = App.useApp()
  const [tab, setTab] = useState('text')
  if (!cell) return null
  const json = prettyJson(cell.value)
  const views: Record<string, string> = { text: cellFullText(cell.value), hex: hexDump(cellBytes(cell.value)) }
  if (json !== null) views.json = json
  const current = views[tab] ?? views.text
  const copy = () =>
    navigator.clipboard.writeText(current).then(
      () => message.success(t('results.viewer.copied')),
      () => message.error(t('results.viewer.copyFailed')),
    )
  return (
    <Modal
      open
      title={t('results.viewer.title', { column: cell.column, row: cell.row })}
      onCancel={onClose}
      width={640}
      footer={
        <Button icon={<IconCopy size={14} />} onClick={copy}>
          {t('results.viewer.copy')}
        </Button>
      }
    >
      <Tabs
        size="small"
        activeKey={views[tab] === undefined ? 'text' : tab}
        onChange={setTab}
        tabBarExtraContent={<span className="muted">{lobNote(cell.value)}</span>}
        items={[
          { key: 'text', label: t('results.viewer.text') },
          { key: 'json', label: 'JSON', disabled: json === null },
          { key: 'hex', label: t('results.viewer.hex') },
        ]}
      />
      <pre className="cell-view">{current}</pre>
    </Modal>
  )
}
