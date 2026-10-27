// 表设计器标签（M3 设计文档第 7 节）：子标签分列、索引、外键、约束、选项、SQL 预览；改动后约 300 毫秒自动预览，
// 校验错误在子标签上显示数目、出错的单元格标红。Ctrl+S 或"保存"打开确认框，执行时进度显示在底部，可以取消。
// 引入 Monaco（SQL 预览），由工作区懒加载。

import { IconDeviceFloppy, IconRefresh, IconTrashX } from '@tabler/icons-react'
import { keepPreviousData, useQuery, useQueryClient } from '@tanstack/react-query'
import { Alert, App, Badge, Button, Progress, Spin, Tabs } from 'antd'
import { type KeyboardEvent, useCallback, useEffect, useMemo, useState } from 'react'
import { useTranslation } from 'react-i18next'

import { useApi } from '../../app/context'
import { useUi } from '../../app/ui'
import { type DesignRequest, type Profile, type TaskStatus, errorText } from '../../shared/api'
import { withVault } from '../connections/vault'
import { useQueryActions } from '../editor/actions'
import { type QueryTab, useTabs } from '../editor/tabs'
import ColumnsPane from './ColumnsPane'
import ConstraintsPane from './ConstraintsPane'
import { useDesigns } from './designStore'
import ForeignKeysPane from './ForeignKeysPane'
import IndexesPane from './IndexesPane'
import { type Draft, type Pane, emptyDraft, fromTable, isDirty, issuesOf, renamesOf, toModel } from './logic'
import OptionsPane from './OptionsPane'
import SaveDialog from './SaveDialog'
import SqlBox from './SqlBox'
import { scriptOf } from './summary'

const NO_ISSUES = issuesOf([], emptyDraft())

export default function DesignView({ tab, profiles }: { tab: QueryTab; profiles: Profile[] }) {
  const api = useApi()
  const { t } = useTranslation()
  const { message, modal } = App.useApp()
  const qc = useQueryClient()
  const { openSql } = useQueryActions()
  const profile = profiles.find((p) => p.id === tab.profileId)
  const target = tab.design!
  const isNew = target.table === null
  const state = useDesigns((s) => s.tabs[tab.id])
  const [schema, setSchema] = useState(target.schema)
  const [pane, setPane] = useState<Pane | 'sql'>('columns')
  const [selected, setSelected] = useState<string | null>(null)
  const [saving, setSaving] = useState(false)
  const [task, setTask] = useState<TaskStatus | null>(null)
  const [loadError, setLoadError] = useState<string | null>(null)

  const caps = useQuery({
    queryKey: ['design-caps', tab.profileId],
    queryFn: () => withVault(api, () => api.designCaps(tab.profileId)),
    enabled: !!profile,
    staleTime: Infinity,
  })

  const load = useCallback(async () => {
    setLoadError(null)
    if (target.table === null) {
      useDesigns.getState().set(tab.id, { original: null, fingerprint: null, storage: null, draft: emptyDraft(), rebuild: false })
      return
    }
    try {
      const r = await withVault(api, () => api.designTable(tab.profileId, target.schema, target.table!))
      useDesigns.getState().set(tab.id, {
        original: fromTable(r.table),
        fingerprint: r.fingerprint,
        storage: r.storage,
        draft: fromTable(r.table),
        rebuild: false,
      })
    } catch (e) {
      setLoadError((e as Error).message)
    }
  }, [api, tab.id, tab.profileId, target.schema, target.table])

  // 第一次打开、新建的表保存后转为编辑这张表时读取
  useEffect(() => {
    const s = useDesigns.getState().tabs[tab.id]
    if (!s || (target.table !== null && s.original === null)) void load()
  }, [tab.id, target.table, load])

  const edit = (fn: (d: Draft) => Draft) => useDesigns.getState().edit(tab.id, fn)
  const dirty = !!state && isDirty(state.draft, state.original)

  const request: DesignRequest | null = useMemo(
    () =>
      state && {
        schema: isNew ? schema : target.schema,
        name: target.table,
        draft: toModel(state.draft),
        renames: renamesOf(state.draft),
        rebuild: state.rebuild,
        fingerprint: state.fingerprint,
      },
    [state, schema, isNew, target.schema, target.table],
  )
  // 改动后约 300 毫秒自动预览
  const [debounced, setDebounced] = useState(request)
  useEffect(() => {
    const h = setTimeout(() => setDebounced(request), 300)
    return () => clearTimeout(h)
  }, [request])
  const preview = useQuery({
    queryKey: ['design-preview', tab.id, JSON.stringify(debounced)],
    queryFn: () => api.previewDesign(tab.profileId, debounced!),
    enabled: !!profile && !!debounced && dirty && !!debounced.schema,
    placeholderData: keepPreviousData,
    retry: false,
  })
  const issues = useMemo(
    () => (state && preview.data && dirty ? issuesOf(preview.data.errors, state.draft) : NO_ISSUES),
    [state, preview.data, dirty],
  )

  // 执行中：轮询任务状态
  useEffect(() => {
    if (!task || task.state !== 'running') return
    const h = setTimeout(async () => {
      try {
        setTask(await api.task(task.id))
      } catch (e) {
        message.error((e as Error).message)
        setTask(null)
      }
    }, 400)
    return () => clearTimeout(h)
  }, [task, api, message])
  // 执行结束：刷新对象树和数据标签的列信息（失败时也可能已执行了一部分）；成功时重新读取这张表（新建的表转为编辑这张表）
  useEffect(() => {
    if (!task || task.state === 'running' || !state) return
    const s = isNew ? schema : target.schema
    useUi.getState().refreshTree(tab.profileId, s)
    void qc.invalidateQueries({ queryKey: ['table-info', tab.profileId, s] })
    if (task.state !== 'done') return
    message.success(t('design.run.done'))
    setTask(null)
    if (isNew) {
      useDesigns.getState().clear(tab.id)
      useTabs.getState().setDesign(tab.id, { schema: s, table: state.draft.name.trim() })
    } else void load()
  }, [task?.state])

  const save = () => {
    if (!dirty || !request || task?.state === 'running') return
    if (!request.schema) {
      setPane('options')
      return
    }
    setSaving(true)
  }
  const onKeyDown = (e: KeyboardEvent) => {
    if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === 's') {
      e.preventDefault()
      save()
    }
  }
  const confirmThen = (title: string, fn: () => void) =>
    dirty ? modal.confirm({ title, okType: 'danger', onOk: fn }) : fn()

  if (loadError) return <Alert type="error" style={{ margin: 12 }} message={loadError} action={<Button onClick={() => void load()}>{t('design.toolbar.reload')}</Button>} />
  if (!state || !profile) return <Spin style={{ margin: 48 }} />

  const count = (p: Pane) => issues.counts[p] ?? 0
  const label = (p: Pane, text: string) => (
    <Badge count={count(p)} size="small" offset={[8, -2]}>
      {text}
    </Badge>
  )
  const running = task?.state === 'running'
  const p = preview.data
  return (
    <div className="design-view" onKeyDown={onKeyDown} tabIndex={-1}>
      <div className="row toolbar">
        <b>{isNew ? t('design.newTable') : `${target.schema}.${target.table}`}</b>
        <span className="muted">{profile.name}</span>
        <span className="grow" />
        <Button size="small" type="text" icon={<IconTrashX size={16} />} disabled={!dirty || running} onClick={() => confirmThen(t('design.toolbar.discardConfirm'), () => void load())}>
          {t('design.toolbar.discard')}
        </Button>
        {!isNew && (
          <Button size="small" type="text" icon={<IconRefresh size={16} />} disabled={running} onClick={() => confirmThen(t('design.toolbar.reloadConfirm'), () => void load())}>
            {t('design.toolbar.reload')}
          </Button>
        )}
        <Button size="small" type="primary" icon={<IconDeviceFloppy size={16} />} disabled={!dirty || running} onClick={save}>
          {t('design.toolbar.save')}
        </Button>
      </div>
      <Tabs
        className="design-tabs grow"
        size="small"
        activeKey={pane}
        onChange={(k) => setPane(k as Pane | 'sql')}
        items={[
          {
            key: 'columns',
            label: label('columns', t('design.tabs.columns')),
            children: (
              <ColumnsPane draft={state.draft} original={state.original} caps={caps.data} issues={issues} selected={selected} onSelect={setSelected} onChange={edit} />
            ),
          },
          {
            key: 'indexes',
            label: label('indexes', t('design.tabs.indexes')),
            children: <IndexesPane draft={state.draft} original={state.original} caps={caps.data} issues={issues} onChange={edit} />,
          },
          {
            key: 'foreignKeys',
            label: label('foreignKeys', t('design.tabs.foreignKeys')),
            children: (
              <ForeignKeysPane profileId={tab.profileId} schema={request?.schema ?? ''} draft={state.draft} original={state.original} caps={caps.data} issues={issues} onChange={edit} />
            ),
          },
          {
            key: 'constraints',
            label: label('constraints', t('design.tabs.constraints')),
            children: <ConstraintsPane draft={state.draft} original={state.original} caps={caps.data} issues={issues} onChange={edit} />,
          },
          {
            key: 'options',
            label: label('options', t('design.tabs.options')),
            children: (
              <OptionsPane
                profileId={tab.profileId}
                schema={request?.schema ?? ''}
                isNew={isNew}
                draft={state.draft}
                storage={state.storage}
                issues={issues}
                onSchema={setSchema}
                onChange={edit}
              />
            ),
          },
          {
            key: 'sql',
            label: t('design.tabs.sql'),
            children: (
              <div className="design-pane design-sql">
                {!dirty ? (
                  <span className="muted">{t('design.sql.nothing')}</span>
                ) : !p ? (
                  <Spin />
                ) : (
                  <>
                    {p.errors.length > 0 && <Alert type="error" message={t('design.sql.errors', { count: p.errors.length })} />}
                    {p.rebuild.active && (
                      <Alert type="warning" message={t('design.sql.rebuild')} description={p.rebuild.reasons.join(' · ') || undefined} />
                    )}
                    {p.rebuild.refused && <Alert type="error" message={t('design.sql.refused', { reason: p.rebuild.refused })} />}
                    {p.warnings.map((w) => (
                      <Alert key={w} type="warning" showIcon message={w} />
                    ))}
                    {p.statements.length > 0 && (
                      <Alert type="info" showIcon message={t(p.transactional ? 'design.sql.transactional' : 'design.sql.autocommit')} />
                    )}
                    <SqlBox sql={scriptOf(p.statements, profile.driver === 'oracle' || profile.driver === 'dameng')} driver={profile.driver} height="calc(100vh - 360px)" />
                  </>
                )}
              </div>
            ),
          },
        ]}
      />
      {task && (
        <div className="design-run">
          {running ? (
            <>
              <span>{t('design.run.running', { done: task.progress.statements_done, total: task.progress.statements_total })}</span>
              <Progress
                size="small"
                style={{ width: 240, margin: 0 }}
                percent={Math.round((100 * task.progress.statements_done) / Math.max(1, task.progress.statements_total))}
              />
              <Button size="small" onClick={() => void api.cancelTask(task.id).catch((e) => message.error((e as Error).message))}>
                {t('design.run.cancel')}
              </Button>
            </>
          ) : (
            <Alert
              type={task.state === 'cancelled' ? 'warning' : 'error'}
              style={{ flex: 1 }}
              closable
              onClose={() => setTask(null)}
              message={task.state === 'cancelled' ? t('design.run.cancelled') : t('design.run.failed')}
              description={
                <>
                  {task.failures[0] && (
                    <div>
                      {t('design.run.failedAt', { n: task.failures[0].index + 1 })}
                      <code className="design-failed-sql">{task.failures[0].text}</code>
                    </div>
                  )}
                  {task.error && <div className="err">{errorText(task.error)}</div>}
                </>
              }
              action={
                task.remaining ? (
                  <Button size="small" onClick={() => openSql(task.remaining ?? '', tab.profileId)}>
                    {t('design.run.remaining')}
                  </Button>
                ) : undefined
              }
            />
          )}
        </div>
      )}
      {saving && request && (
        <SaveDialog
          profile={profile}
          request={request}
          onRebuild={(on) => useDesigns.getState().setRebuild(tab.id, on)}
          onReload={() => {
            setSaving(false)
            void load()
          }}
          onClose={() => setSaving(false)}
          onStarted={(id) => setTask({ id, state: 'running', progress: { statements_done: 0, statements_total: 0 } } as TaskStatus)}
        />
      )}
    </div>
  )
}
