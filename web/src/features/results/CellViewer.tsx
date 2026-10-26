// 单元格查看对话框（设计稿第 3 部分）：文本 / JSON / 十六进制，可复制。

import { App, Button, Modal, Tabs } from 'antd'
import { IconCopy } from '@tabler/icons-react'
import { useState } from 'react'

import type { CellValue } from '../../shared/exec'
import { cellBytes, cellFullText, hexDump, lobNote, prettyJson } from './cell'

export interface ViewedCell {
  column: string
  /** 从 1 开始的行号。 */
  row: number
  value: CellValue
}

export default function CellViewer({ cell, onClose }: { cell: ViewedCell | null; onClose(): void }) {
  const { message } = App.useApp()
  const [tab, setTab] = useState('text')
  if (!cell) return null
  const json = prettyJson(cell.value)
  const views: Record<string, string> = { text: cellFullText(cell.value), hex: hexDump(cellBytes(cell.value)) }
  if (json !== null) views.json = json
  const current = views[tab] ?? views.text
  const copy = () =>
    navigator.clipboard.writeText(current).then(
      () => message.success('已复制'),
      () => message.error('复制失败'),
    )
  return (
    <Modal
      open
      title={`${cell.column} · 第 ${cell.row} 行`}
      onCancel={onClose}
      width={640}
      footer={
        <Button icon={<IconCopy size={14} />} onClick={copy}>
          复制
        </Button>
      }
    >
      <Tabs
        size="small"
        activeKey={views[tab] === undefined ? 'text' : tab}
        onChange={setTab}
        tabBarExtraContent={<span className="muted">{lobNote(cell.value)}</span>}
        items={[
          { key: 'text', label: '文本' },
          { key: 'json', label: 'JSON', disabled: json === null },
          { key: 'hex', label: '十六进制' },
        ]}
      />
      <pre className="cell-view">{current}</pre>
    </Modal>
  )
}
