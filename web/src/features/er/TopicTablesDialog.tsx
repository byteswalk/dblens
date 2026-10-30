// 编辑一个业务主题包含哪些表：左边是其余的表，右边是这个主题的表。

import { Modal, Transfer } from 'antd'
import { useMemo, useState } from 'react'
import { useTranslation } from 'react-i18next'

import type { ErTable } from '../../shared/api'
import type { Topic } from './topics'

export interface TopicTablesDialogProps {
  topic: Topic
  tables: ErTable[]
  /** 表名 → 现在所在主题的显示名。 */
  groups: Map<string, string>
  onOk(names: string[]): void
  onCancel(): void
}

export default function TopicTablesDialog({ topic, tables, groups, onOk, onCancel }: TopicTablesDialogProps) {
  const { t } = useTranslation()
  const [picked, setPicked] = useState(topic.tables)
  const data = useMemo(() => tables.map((x) => ({ key: x.table.name, comment: x.table.comment ?? '' })), [tables])
  return (
    <Modal
      open
      width={760}
      title={t('er.groups.editTables', { name: topic.name })}
      onCancel={onCancel}
      onOk={() => onOk(picked)}
    >
      <Transfer
        dataSource={data}
        targetKeys={picked}
        onChange={(keys) => setPicked(keys.map(String))}
        titles={[t('er.groups.otherTables'), topic.name]}
        showSearch
        filterOption={(q, x) => `${x.key} ${x.comment}`.toLowerCase().includes(q.toLowerCase())}
        pagination={{ pageSize: 100 }}
        listStyle={{ width: 340, height: 420 }}
        render={(x) => {
          const from = picked.includes(x.key) ? '' : groups.get(x.key)
          return (
            <span title={x.comment}>
              {x.key}
              {from && from !== topic.name ? <span className="muted"> · {from}</span> : null}
            </span>
          )
        }}
      />
    </Modal>
  )
}
