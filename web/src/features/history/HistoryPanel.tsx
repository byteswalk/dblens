// 历史侧栏（M1c1 设计文档第 6 节）：搜索、按连接和失败过滤、按日期分组、滚动加载。
// 双击插入到当前编辑器；右键在新标签中打开、复制、加入收藏、删除。历史由服务端在执行时写入，执行结束后自动刷新。

import { App, Button, Dropdown, Input, Select, Switch, Tooltip } from 'antd'
import { IconSearch } from '@tabler/icons-react'
import { useInfiniteQuery, useQuery, useQueryClient } from '@tanstack/react-query'
import { useEffect, useMemo, useState } from 'react'
import { useTranslation } from 'react-i18next'

import { useApi } from '../../app/context'
import DbIcon from '../../shared/DbIcon'
import type { HistoryItem } from '../../shared/api'
import { useQueryActions } from '../editor/actions'
import { useFavoriteDialog } from '../favorites/dialog'
import { PAGE, detail, firstLine, groupByDay } from './history'

const ENV_COLORS: Record<string, string> = {
  dev: 'var(--env-dev)',
  test: 'var(--env-test)',
  prod: 'var(--env-prod)',
  none: 'var(--env-none)',
}

/** 输入停顿 ms 毫秒后才更新。 */
function useDebounced<T>(value: T, ms: number): T {
  const [v, setV] = useState(value)
  useEffect(() => {
    const timer = setTimeout(() => setV(value), ms)
    return () => clearTimeout(timer)
  }, [value, ms])
  return v
}

export default function HistoryPanel() {
  const api = useApi()
  const qc = useQueryClient()
  const { t } = useTranslation()
  const { message } = App.useApp()
  const { insertSql, openSql } = useQueryActions()
  const { data: profiles = [] } = useQuery({ queryKey: ['profiles'], queryFn: () => api.profiles() })
  const [text, setText] = useState('')
  const [profile, setProfile] = useState<string>('')
  const [failed, setFailed] = useState(false)
  const q = useDebounced(text.trim(), 300)

  const query = useInfiniteQuery({
    queryKey: ['history', { q, profile, failed }],
    queryFn: ({ pageParam }) =>
      api.history({ q: q || undefined, profile: profile || undefined, failed, before: pageParam, limit: PAGE }),
    initialPageParam: undefined as number | undefined,
    getNextPageParam: (last) => (last.length === PAGE ? last[last.length - 1].id : undefined),
  })
  const items = useMemo(() => query.data?.pages.flat() ?? [], [query.data])
  const groups = useMemo(() => groupByDay(items), [items])

  const remove = async (item: HistoryItem) => {
    try {
      await api.deleteHistory(item.id)
      await qc.invalidateQueries({ queryKey: ['history'] })
    } catch (e) {
      message.error((e as Error).message)
    }
  }

  const menu = (item: HistoryItem) => ({
    items: [
      { key: 'open', label: t('app.history.openInTab') },
      { key: 'copy', label: t('app.history.copy') },
      { key: 'favorite', label: t('app.history.addFavorite') },
      { type: 'divider' as const },
      { key: 'delete', label: t('app.history.delete'), danger: true },
    ],
    onClick: ({ key }: { key: string }) => {
      if (key === 'open') openSql(item.sql, item.profile_id)
      else if (key === 'copy') void navigator.clipboard.writeText(item.sql)
      else if (key === 'favorite') {
        useFavoriteDialog.getState().open({ mode: 'create', kind: 'item', sql: item.sql, driver: item.driver, variant: item.variant })
      }
      else if (key === 'delete') void remove(item)
    },
  })

  return (
    <div className="hist">
      <div className="hist-filter">
        <Input
          size="small"
          allowClear
          prefix={<IconSearch size={16} />}
          placeholder={t('app.history.search')}
          value={text}
          onChange={(e) => setText(e.target.value)}
        />
        <div className="row" style={{ gap: 8 }}>
          <Select
            size="small"
            className="grow"
            value={profile}
            onChange={setProfile}
            popupMatchSelectWidth={false}
            options={[
              { value: '', label: t('app.history.allConnections') },
              ...profiles.map((p) => ({ value: p.id, label: p.name })),
            ]}
          />
          <span className="row" style={{ gap: 4 }}>
            <Switch size="small" checked={failed} onChange={setFailed} />
            {t('app.history.failedOnly')}
          </span>
        </div>
      </div>
      <div
        className="hist-list"
        onScroll={(e) => {
          const el = e.currentTarget
          if (el.scrollTop + el.clientHeight > el.scrollHeight - 80 && query.hasNextPage && !query.isFetchingNextPage) {
            void query.fetchNextPage()
          }
        }}
      >
        {query.error ? (
          <div className="hist-empty">
            <div className="err">{(query.error as Error).message}</div>
            <Button size="small" onClick={() => void query.refetch()}>
              {t('app.history.retry')}
            </Button>
          </div>
        ) : !query.isPending && items.length === 0 ? (
          <div className="hist-empty muted">{t('app.history.empty')}</div>
        ) : (
          groups.map((g) => (
            <div key={g.day}>
              <div className="hist-day">{g.label}</div>
              {g.items.map((item) => {
                const p = profiles.find((x) => x.id === item.profile_id)
                return (
                  <Dropdown key={item.id} trigger={['contextMenu']} menu={menu(item)}>
                    <Tooltip
                      placement="right"
                      mouseEnterDelay={0.6}
                      title={<pre className="hist-preview">{item.sql}</pre>}
                      styles={{ root: { maxWidth: 560 } }}
                    >
                      <div className="hist-item" onDoubleClick={() => insertSql(item.sql, item.profile_id)}>
                        <div className="hist-sql">{firstLine(item.sql)}</div>
                        <div className="row hist-meta">
                          <span className="dot" style={{ background: ENV_COLORS[p?.environment ?? 'none'] }} />
                          <DbIcon driver={item.driver} variant={item.variant} />
                          <span className="hist-conn">{item.profile_name}</span>
                          <span className="grow" />
                          <span>{detail(item)}</span>
                        </div>
                        {item.error && <div className="hist-err">{item.error}</div>}
                      </div>
                    </Tooltip>
                  </Dropdown>
                )
              })}
            </div>
          ))
        )}
      </div>
    </div>
  )
}
