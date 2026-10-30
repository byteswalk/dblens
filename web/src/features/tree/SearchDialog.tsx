// 全库查找：在连接的一个 schema 里按表名、列名和注释查表和列，双击打开表数据。

import { useQuery } from '@tanstack/react-query'
import { Alert, Input, Modal, Select, Table } from 'antd'
import { useEffect, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { create } from 'zustand'

import { useApi } from '../../app/context'
import type { SearchHit } from '../../shared/api'
import { schemaOptions } from '../../shared/schemaOptions'
import { useTabs } from '../editor/tabs'

const LIMIT = 500

interface SearchDialogState {
  profileId: string | null
  open(profileId: string): void
  close(): void
}

export const useSearchDialog = create<SearchDialogState>()((set) => ({
  profileId: null,
  open: (profileId) => set({ profileId }),
  close: () => set({ profileId: null }),
}))

export default function SearchDialog() {
  const { t } = useTranslation()
  const api = useApi()
  const { profileId, close } = useSearchDialog()
  const [selected, setSelected] = useState<string | null>(null)
  const [schema, setSchema] = useState<string | null>(null)
  const [text, setText] = useState('')
  const [query, setQuery] = useState('')
  useEffect(() => {
    setSelected(profileId)
    setSchema(null)
    setQuery('')
  }, [profileId])

  const { data: profiles = [] } = useQuery({ queryKey: ['profiles'], queryFn: () => api.profiles() })
  const profile = profiles.find((p) => p.id === selected)
  const schemas = useQuery({
    queryKey: ['search-schemas', selected],
    queryFn: () => schemaOptions(api, profile!),
    enabled: !!profile,
  })
  const hits = useQuery({
    queryKey: ['search', selected, schema, query],
    queryFn: () => api.search(selected!, schema!, query),
    enabled: !!selected && !!schema && !!query,
    retry: false,
  })

  const openHit = (hit: SearchHit) => {
    if (!selected || !schema) return
    useTabs.getState().openData(selected, { schema, object: hit.table, objectKind: 'table' })
    close()
  }

  return (
    <Modal open={profileId !== null} title={t('tree.search.title')} onCancel={close} footer={null} width={720}>
      <div className="row" style={{ gap: 8, marginBottom: 12 }}>
        <Select
          aria-label={t('tree.search.connection')}
          value={selected}
          onChange={(v) => {
            setSelected(v)
            setSchema(null)
          }}
          options={profiles.map((p) => ({ value: p.id, label: p.name }))}
          style={{ width: 160 }}
        />
        <Select
          aria-label={t('tree.search.schema')}
          placeholder={t('tree.search.schema')}
          showSearch
          value={schema}
          onChange={setSchema}
          loading={schemas.isFetching}
          options={schemas.data ?? []}
          optionFilterProp="label"
          style={{ width: 180 }}
        />
        <Input.Search
          className="grow"
          placeholder={t('tree.search.placeholder')}
          value={text}
          onChange={(e) => setText(e.target.value)}
          onSearch={(v) => setQuery(v.trim())}
          loading={hits.isFetching}
          allowClear
        />
      </div>
      {hits.error && <Alert type="error" message={(hits.error as Error).message} style={{ marginBottom: 8 }} />}
      {hits.data?.length === LIMIT && <Alert type="info" message={t('tree.search.limit')} style={{ marginBottom: 8 }} />}
      <Table<SearchHit>
        size="small"
        rowKey={(h) => `${h.table}\u0000${h.column ?? ''}`}
        dataSource={hits.data ?? []}
        pagination={{ pageSize: 12, showSizeChanger: false, hideOnSinglePage: true }}
        onRow={(h) => ({ onDoubleClick: () => openHit(h), title: t('tree.search.open') })}
        columns={[
          { title: t('tree.search.colTable'), dataIndex: 'table' },
          {
            title: t('tree.search.colColumn'),
            dataIndex: 'column',
            render: (c: string | null) => c ?? <span className="muted">{t('tree.search.tableHit')}</span>,
          },
          { title: t('tree.search.colComment'), dataIndex: 'comment', ellipsis: true },
        ]}
      />
    </Modal>
  )
}
