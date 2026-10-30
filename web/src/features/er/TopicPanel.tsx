// 画布一角的业务主题列表：点击只看这个主题，双击改名，可新建、删除。

import { IconPlus, IconRestore, IconX } from '@tabler/icons-react'
import { Input } from 'antd'
import { useState } from 'react'
import { useTranslation } from 'react-i18next'

import { OTHER_TOPIC, type Topic } from './topics'

export interface TopicPanelProps {
  topics: Topic[]
  current: string | null
  onSelect(id: string | null): void
  onRename(id: string, name: string): void
  onAdd(): void
  onDelete(id: string): void
  onReset(): void
}

export default function TopicPanel({ topics, current, onSelect, onRename, onAdd, onDelete, onReset }: TopicPanelProps) {
  const { t } = useTranslation()
  const [editing, setEditing] = useState<string | null>(null)
  const total = topics.reduce((n, x) => n + x.tables.length, 0)
  return (
    <div className="er-topics">
      <div className="row er-topics-h">
        <span className="grow">{t('er.groups.label')}</span>
        <span className="er-topic-btn" title={t('er.groups.add')} onClick={onAdd}>
          <IconPlus size={14} />
        </span>
        <span className="er-topic-btn" title={t('er.groups.reset')} onClick={onReset}>
          <IconRestore size={14} />
        </span>
      </div>
      <div className={`row er-topic${current === null ? ' on' : ''}`} onClick={() => onSelect(null)}>
        <span className="grow">{t('er.groups.all')}</span>
        <span className="muted">{total}</span>
      </div>
      <div className="er-topics-list">
        {topics.map((x) => (
          <div
            key={x.id}
            className={`row er-topic${current === x.id ? ' on' : ''}`}
            onClick={() => onSelect(x.id)}
            onDoubleClick={() => setEditing(x.id)}
            title={t('er.groups.editHint')}
          >
            {editing === x.id ? (
              <Input
                size="small"
                autoFocus
                defaultValue={x.name}
                onClick={(e) => e.stopPropagation()}
                onBlur={(e) => {
                  setEditing(null)
                  if (e.target.value.trim()) onRename(x.id, e.target.value.trim())
                }}
                onPressEnter={(e) => e.currentTarget.blur()}
              />
            ) : (
              <>
                <span className="grow er-topic-name">{x.name}</span>
                <span className="muted">{x.tables.length}</span>
                {x.id !== OTHER_TOPIC && (
                  <span
                    className="er-topic-btn del"
                    title={t('er.groups.delete')}
                    onClick={(e) => {
                      e.stopPropagation()
                      onDelete(x.id)
                    }}
                  >
                    <IconX size={12} />
                  </span>
                )}
              </>
            )}
          </div>
        ))}
      </div>
    </div>
  )
}
