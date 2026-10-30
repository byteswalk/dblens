// 工作区（设计稿第 2 部分）：标签栏和当前标签页（查询标签或数据标签）。标签顶部有一条环境色线，生产连接的标签整块标红、
// 标题带"生产"。懒加载：Monaco 和结果表格只在打开第一个查询标签时才加载。
// 标签多时先缩窄（悬停看全名），再放不下就用滚轮横向滚动，不显示滚动条；右端 ▾ 列出全部标签；右键菜单批量关闭。

import { Dropdown, type MenuProps } from 'antd'
import { IconAffiliate, IconArrowsDiff, IconTransfer, IconChevronDown, IconClipboardCheck, IconFileCode, IconPlus, IconSchema, IconTable, IconTableOptions, IconX } from '@tabler/icons-react'
import { useQuery } from '@tanstack/react-query'
import { Spin } from 'antd'
import { Suspense, type WheelEvent, lazy, useEffect, useRef } from 'react'
import { useTranslation } from 'react-i18next'

import { useApi } from '../../app/context'
import DbIcon from '../../shared/DbIcon'
import type { Profile } from '../../shared/api'
import DataView from '../data/DataView'
import QueryView from './QueryView'
import { disposeModel } from './SqlEditor'
import { useQueryActions } from './actions'
import { useTabRuntime } from './runner'
import { type QueryTab, useTabs, wheelTarget } from './tabs'

// 设计器带 Monaco 的 SQL 预览，打开第一个设计器标签时才加载
const DesignView = lazy(() => import('../design/DesignView'))
// 比对标签的同步预览带 Monaco，同样懒加载
const CompareView = lazy(() => import('../compare/CompareView'))
// ER 图带 React Flow，打开第一张图时才加载
const ErView = lazy(() => import('../er/ErView'))
// 迁移标签的 DDL 预览带 Monaco
const MigrateView = lazy(() => import('../migrate/MigrateView'))
// 规范检查：规范库和结果页
const LintLibraryView = lazy(() => import('../lint/LibraryView'))
const LintRunView = lazy(() => import('../lint/RunView'))

/** 按住标签横向拖动：移过别的标签时换位。不用 HTML5 拖放：Tauri 的窗口拖放会拦截它。 */
function startTabDrag(id: string, startX: number) {
  let dragging = false
  const move = (e: globalThis.MouseEvent) => {
    if (!dragging && Math.abs(e.clientX - startX) < 6) return
    dragging = true
    document.body.style.cursor = 'grabbing'
    const over = document.elementFromPoint(e.clientX, e.clientY)?.closest<HTMLElement>('[data-tab-id]')?.dataset.tabId
    if (over && over !== id) useTabs.getState().moveTab(id, over)
  }
  const up = () => {
    document.body.style.cursor = ''
    window.removeEventListener('mousemove', move)
    window.removeEventListener('mouseup', up)
  }
  window.addEventListener('mousemove', move)
  window.addEventListener('mouseup', up)
}

function TabTitle({ tab, active, envClass, profileName, profile }: {
  tab: QueryTab
  active: boolean
  envClass: string
  profileName: string
  profile: Profile | undefined
}) {
  const { t } = useTranslation()
  const rt = useTabRuntime(tab.id)
  const { closeQuery, closeMany } = useQueryActions()
  const menu: MenuProps = {
    items: [
      { key: 'close', label: t('editor.tab.close') },
      { key: 'others', label: t('editor.tab.closeOthers') },
      { key: 'right', label: t('editor.tab.closeRight') },
      { key: 'all', label: t('editor.tab.closeAll') },
    ],
    onClick: ({ key }) => {
      const ids = useTabs.getState().tabs.map((x) => x.id)
      const i = ids.indexOf(tab.id)
      if (key === 'close') void closeQuery(tab.id)
      else if (key === 'others') void closeMany(ids.filter((id) => id !== tab.id))
      else if (key === 'right') void closeMany(ids.slice(i + 1))
      else void closeMany(ids)
    },
  }
  return (
    <Dropdown trigger={['contextMenu']} menu={menu}>
      <div
        className={`tab ${envClass}${active ? ' on' : ''}`}
        onClick={() => useTabs.getState().activate(tab.id)}
        onMouseDown={(e) => {
          // 中键关闭
          if (e.button === 1) {
            e.preventDefault()
            void closeQuery(tab.id)
          }
          if (e.button === 0 && !(e.target as HTMLElement).closest('.x')) startTabDrag(tab.id, e.clientX)
        }}
        title={`${tab.title} · ${profileName}`}
        data-tab-id={tab.id}
      >
        {tab.lint ? (
          <IconClipboardCheck size={16} />
        ) : tab.er ? (
          <IconAffiliate size={16} />
        ) : tab.migrate ? (
          <IconTransfer size={16} />
        ) : tab.compare ? (
          <IconArrowsDiff size={16} />
        ) : tab.design?.model ? (
          <IconSchema size={16} />
        ) : tab.design ? (
          <IconTableOptions size={16} />
        ) : tab.data ? (
          <IconTable size={16} />
        ) : profile ? (
          <DbIcon driver={profile.driver} variant={rt.session?.variant ?? profile.variant} />
        ) : (
          <IconFileCode size={16} />
        )}
        <span className="tab-title">{tab.title}</span>
        {envClass === 'prod' ? (
          <span className="tab-sub">· {t('editor.tab.prod')}</span>
        ) : (
          <span className="tab-sub muted">{profileName}</span>
        )}
        {rt.running && <span className="running-dot" />}
        <span
          className="x"
          onClick={(e) => {
            e.stopPropagation()
            void closeQuery(tab.id)
          }}
        >
          <IconX size={14} />
        </span>
      </div>
    </Dropdown>
  )
}

export default function Workspace() {
  const api = useApi()
  const { t } = useTranslation()
  const { tabs, active } = useTabs()
  const { newQuery } = useQueryActions()
  const { data: profiles = [] } = useQuery({ queryKey: ['profiles'], queryFn: () => api.profiles() })

  // 关闭的标签页：删除它的编辑器模型
  const known = useRef(new Set<string>())
  useEffect(() => {
    const now = new Set(tabs.map((x) => x.id))
    for (const id of known.current) if (!now.has(id)) disposeModel(id)
    known.current = now
  }, [tabs])

  // 切换或新开标签时把当前标签滚到可见处
  const strip = useRef<HTMLDivElement>(null)
  useEffect(() => {
    strip.current?.querySelector('.tab.on')?.scrollIntoView({ block: 'nearest', inline: 'nearest' })
  }, [active, tabs.length])
  // 竖向滚轮横向滚动标签
  const onWheel = (e: WheelEvent) => {
    const el = strip.current
    if (!el || e.deltaX !== 0 || e.deltaY === 0) return
    const tabs = [...el.children] as HTMLElement[]
    const on = tabs.findIndex((c) => c.classList.contains('on'))
    const lefts = tabs.map((c) => c.offsetLeft - el.offsetLeft)
    const next = wheelTarget(lefts, on, on < 0 ? 0 : tabs[on].offsetWidth, el.scrollLeft, e.deltaY > 0)
    if (next !== undefined) el.scrollLeft = next
  }

  // 模型的设计器标签、比对标签不绑定连接
  const profileName = (id: string) => (id ? profiles.find((x) => x.id === id)?.name ?? t('editor.tab.profileDeleted') : '')
  const list: MenuProps = {
    items: tabs.map((tb) => ({ key: tb.id, label: `${tb.title} · ${profileName(tb.profileId)}` })),
    selectable: true,
    selectedKeys: active ? [active] : [],
    onClick: ({ key }) => useTabs.getState().activate(key),
    style: { maxHeight: 480, overflowY: 'auto' },
  }

  const tab = tabs.find((x) => x.id === active)
  return (
    <div className="workspace">
      <div className="row tabs">
        <div className="row tab-strip" ref={strip} onWheel={onWheel}>
          {tabs.map((tb) => {
            const p = profiles.find((x) => x.id === tb.profileId)
            return (
              <TabTitle
                key={tb.id}
                tab={tb}
                active={tb.id === active}
                envClass={p?.environment && p.environment !== 'none' ? p.environment : ''}
                profileName={profileName(tb.profileId)}
                profile={p}
              />
            )
          })}
        </div>
        <div className="tab add" title={t('editor.tab.newQuery')} onClick={() => newQuery()}>
          <IconPlus size={16} />
        </div>
        <span className="grow" />
        {tabs.length > 0 && (
          <Dropdown trigger={['click']} menu={list}>
            <div className="tab add list" title={t('editor.tab.list')}>
              <IconChevronDown size={16} />
            </div>
          </Dropdown>
        )}
      </div>
      {tab &&
        (tab.lint?.view === 'library' ? (
          <Suspense fallback={<Spin style={{ margin: 48 }} />}>
            <LintLibraryView key={tab.id} />
          </Suspense>
        ) : tab.lint?.view === 'run' ? (
          <Suspense fallback={<Spin style={{ margin: 48 }} />}>
            <LintRunView key={tab.id} tab={tab} />
          </Suspense>
        ) : tab.migrate ? (
          <Suspense fallback={<Spin style={{ margin: 48 }} />}>
            <MigrateView key={tab.id} tab={tab} profiles={profiles} />
          </Suspense>
        ) : tab.er ? (
          <Suspense fallback={<Spin style={{ margin: 48 }} />}>
            <ErView key={tab.id} tab={tab} />
          </Suspense>
        ) : tab.compare ? (
          <Suspense fallback={<Spin style={{ margin: 48 }} />}>
            <CompareView key={tab.id} tab={tab} profiles={profiles} />
          </Suspense>
        ) : tab.design ? (
          <Suspense fallback={<Spin style={{ margin: 48 }} />}>
            <DesignView key={tab.id} tab={tab} profiles={profiles} />
          </Suspense>
        ) : tab.data ? (
          <DataView key={tab.id} tab={tab} profiles={profiles} />
        ) : (
          <QueryView tab={tab} profiles={profiles} />
        ))}
    </div>
  )
}
