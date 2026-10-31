// 一个查询标签页的内容：工具栏、提示条、编辑器和结果面板（可拖动分隔线，位置存进设置）、确认框。
// 处理编辑器命令：执行当前语句（有选中时执行选中部分）、执行全部、停止、格式化、执行计划。

import { useQueryClient } from '@tanstack/react-query'
import { Alert, App, Button } from 'antd'
import { useCallback, useEffect, useMemo, useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { Group, Panel, Separator, useDefaultLayout } from 'react-resizable-panels'

import { useApi } from '../../app/context'
import { type Profile, errorText } from '../../shared/api'
import { hasCatalogs } from '../../shared/schemaOptions'
import { layoutStorage } from '../../shared/settings'
import ResultsPanel from '../results/ResultsPanel'
import { useExportDialog } from '../export/dialog'
import { useFavoriteDialog } from '../favorites/dialog'
import { sqlDialect } from '../sql/dialects'
import { defaultSchema, metaKey, metaReader } from '../sql/metadata'
import { sqlClient } from '../sql/service'
import { useSyntax } from '../sql/syntax'
import ConfirmDialog from './ConfirmDialog'
import Toolbar from './Toolbar'
import { onCommand, onInsert } from './commands'
import { editorLanguage, formatSql } from './dialect'
import { errorMarks } from './markers'
import { runnerFor, useTabRuntime } from './runner'
import SqlEditor, { type MonacoEditor } from './SqlEditor'
import { type QueryTab, takeRun, useTabs } from './tabs'

/** 会改变对象结构的语句（按首个关键字）。 */
const DDL = new Set(['CREATE', 'ALTER', 'DROP', 'RENAME', 'TRUNCATE', 'COMMENT'])

/** 自动刷新只重跑这些语句。 */
const REFRESHABLE = new Set(['SELECT', 'WITH', 'SHOW'])

export default function QueryView({ tab, profiles }: { tab: QueryTab; profiles: Profile[] }) {
  const api = useApi()
  const qc = useQueryClient()
  const { t } = useTranslation()
  const { message, modal } = App.useApp()
  const runner = runnerFor(api, tab.id)
  const rt = useTabRuntime(tab.id)
  const profile = profiles.find((p) => p.id === tab.profileId)
  const editor = useRef<MonacoEditor | null>(null)
  const layout = useDefaultLayout({ id: 'query', storage: layoutStorage })
  const onReady = useCallback((e: MonacoEditor | null) => {
    editor.current = e
  }, [])

  useEffect(
    () =>
      onCommand((cmd) => {
        const ed = editor.current
        const model = ed?.getModel()
        if (!ed || !model) return
        const sel = ed.getSelection()
        const selected = sel && !sel.isEmpty() ? sel : null
        const part = () =>
          selected
            ? { sql: model.getValueInRange(selected), base: model.getOffsetAt(selected.getStartPosition()) }
            : null
        const cursor = model.getOffsetAt(ed.getPosition() ?? { lineNumber: 1, column: 1 })
        const variant = rt.session?.variant ?? profile?.variant ?? null
        switch (cmd) {
          case 'run-current':
          case 'explain': {
            const explain = cmd === 'explain'
            const p = part()
            if (p) void runner.run(p.sql, p.base, { explain }, model.getValue())
            else void runner.run(model.getValue(), 0, { cursor, explain })
            break
          }
          case 'run-all': {
            const p = part() ?? { sql: model.getValue(), base: 0 }
            void runner.run(p.sql, p.base, {}, model.getValue())
            break
          }
          case 'cancel':
            runner.cancel()
            break
          case 'favorite': {
            // 收藏选中的部分，没有选中时收藏光标所在的语句（解析器不可用时收藏全文）
            const text = selected ? model.getValueInRange(selected) : null
            void (async () => {
              let sql = text
              if (!sql) {
                const all = model.getValue()
                const r = profile
                  ? await sqlClient.statementAt(sqlDialect(profile.driver, variant, rt.session?.compat).dialect, all, cursor)
                  : null
                sql = r?.ok && r.value ? all.slice(r.value.start, r.value.end) : all
              }
              sql = sql.trim()
              if (!sql) message.info(t('app.favorites.nothing'))
              else useFavoriteDialog.getState().open({ mode: 'create', kind: 'item', sql, driver: profile?.driver, variant })
            })()
            break
          }
          case 'first-problem': {
            const first = useSyntax.getState().tabs[tab.id]?.first
            if (first) {
              const pos = { lineNumber: first.line, column: first.column }
              ed.setPosition(pos)
              ed.revealPositionInCenter(pos)
              ed.focus()
            }
            break
          }
          case 'format': {
            if (!profile) return
            const range = selected ?? model.getFullModelRange()
            try {
              const text = formatSql(model.getValueInRange(range), profile.driver, rt.session?.variant)
              ed.executeEdits('format', [{ range, text }])
            } catch (e) {
              message.error(t('editor.query.formatFailed', { message: e instanceof Error ? e.message : String(e) }))
            }
            break
          }
        }
      }),
    [runner, profile, rt.session?.variant, message, t, tab.id],
  )

  // 运维查询打开的标签：打开后立即执行全文
  useEffect(() => {
    if (takeRun(tab.id)) void runner.run(tab.sql, 0)
  }, [tab.id]) // 只在标签第一次显示时检查

  // 协议层面的错误（如上一段还没执行完）用全局提示（M1 设计文档第 5 节），同时记在"消息"里
  const protocolError = rt.exec.protocolError
  useEffect(() => {
    if (protocolError) message.error(errorText(protocolError))
  }, [protocolError, message])

  const marks = useMemo(() => errorMarks(rt.exec, rt.source), [rt.exec, rt.source])

  // 历史、收藏侧栏插入 SQL
  useEffect(
    () =>
      onInsert((sql) => {
        const ed = editor.current
        const sel = ed?.getSelection()
        const model = ed?.getModel()
        if (!ed || !sel || !model) return
        // 光标前、后已有内容时各另起一行，不和原有的 SQL 连在一起
        const before = model.getLineContent(sel.startLineNumber).slice(0, sel.startColumn - 1)
        const after = model.getLineContent(sel.endLineNumber).slice(sel.endColumn - 1)
        const text = `${before.trim() ? '\n' : ''}${sql}${after.trim() ? '\n' : ''}`
        ed.executeEdits('insert', [{ range: sel, text, forceMoveMarkers: true }])
        ed.focus()
      }),
    [],
  )

  // 执行结束：刷新历史侧栏（服务端在后台写入历史，稍等片刻）；
  // 执行成功的语句中有 DDL 时，补全用的元数据（连同后端的元数据缓存）失效，新建的表马上能补全
  const refreshed = useRef<unknown>(null)
  useEffect(() => {
    if (rt.running || !profile || refreshed.current === rt.exec) return
    refreshed.current = rt.exec
    if (rt.exec.statements.length) setTimeout(() => void qc.invalidateQueries({ queryKey: ['history'] }), 300)
    if (!rt.exec.statements.some((s) => !s.error && DDL.has(s.keyword))) return
    void api
      .refresh(profile.id, null)
      .catch(() => {})
      .then(() => qc.invalidateQueries({ queryKey: metaKey(profile.id) }))
  }, [rt.running, rt.exec, profile, api, qc])
  // SQL Server：会话的当前 schema 是库，补全在这个库的架构中查找
  const catalog = profile && hasCatalogs(profile.driver) ? (rt.session?.schema ?? profile.database) : null
  const meta = useMemo(
    () => (profile ? metaReader(qc, api, profile.id, profile.driver, catalog) : null),
    [qc, api, profile?.id, profile?.driver, catalog],
  )
  const sql = useMemo(
    () =>
      profile && meta
        ? {
            driver: profile.driver,
            variant: rt.session?.variant ?? profile.variant,
            compat: rt.session?.compat,
            schema:
              hasCatalogs(profile.driver)
                ? defaultSchema(profile.driver, profile.user, profile.database)
                : (rt.session?.schema ?? defaultSchema(profile.driver, profile.user, profile.database)),
            meta,
          }
        : null,
    [profile, meta, rt.session?.variant, rt.session?.compat, rt.session?.schema],
  )
  const confirm = !rt.running && !rt.declined ? rt.exec.confirm : null
  const sub = rt.submission

  // 自动刷新：上一轮结束后隔 refresh 秒重跑同样的语句。只重跑查询，出错或有写操作时关掉
  const [refresh, setRefresh] = useState(0)
  const changeRefresh = (seconds: number) => {
    if (seconds && !refresh && profile?.environment === 'prod') {
      modal.confirm({
        title: t('editor.refresh.prodTitle'),
        content: t('editor.refresh.prodContent', { seconds }),
        onOk: () => setRefresh(seconds),
      })
    } else setRefresh(seconds)
  }
  const idle = !rt.running || rt.exec.waitingForMore
  useEffect(() => {
    if (!refresh || !idle || !sub) return
    if (!rt.exec.statements.every((s) => !s.error && REFRESHABLE.has(s.keyword))) {
      setRefresh(0)
      message.warning(t('editor.refresh.stopped'))
      return
    }
    const timer = setTimeout(() => {
      void runner.stop().then(() => runner.run(sub.sql, sub.base, sub.opts, rt.source))
    }, refresh * 1000)
    return () => clearTimeout(timer)
  }, [refresh, idle, sub, rt.exec, rt.source, runner, message, t])

  return (
    <div className="query">
      <Toolbar tab={tab} rt={rt} runner={runner} profiles={profiles} refresh={refresh} onRefreshChange={changeRefresh} />
      {!profile && <Alert type="error" banner message={t('editor.query.profileDeleted')} />}
      {rt.session?.broken && (
        <Alert
          type="warning"
          banner
          message={t('editor.query.disconnected')}
          action={
            <Button size="small" onClick={() => runner.reconnect().catch((e: Error) => message.error(e.message))}>
              {t('editor.query.reconnect')}
            </Button>
          }
        />
      )}
      <Group orientation="vertical" className="grow" {...layout}>
        <Panel id="editor" minSize={80}>
          <SqlEditor
            path={tab.id}
            language={profile ? editorLanguage(profile.driver, rt.session?.compat) : 'sql'}
            defaultValue={tab.sql}
            marks={marks}
            sql={sql}
            onChange={(v) => useTabs.getState().setSql(tab.id, v)}
            onReady={onReady}
          />
        </Panel>
        <Separator className="splitter-h" />
        <Panel id="results" defaultSize="40%" minSize={60}>
          <ResultsPanel
            rt={rt}
            onFetchMore={() => runner.fetchMore()}
            onExport={(i) => {
              // 导出这条语句的原文（取自执行时的全文）
              const s = rt.exec.statements[i]
              useExportDialog.getState().open({
                profileId: tab.profileId,
                source: { sql: rt.source.slice(s.start, s.end) },
                baseName: 'export',
                inTransaction: !!rt.session?.in_transaction,
              })
            }}
          />
        </Panel>
      </Group>
      <ConfirmDialog
        open={confirm !== null}
        sql={confirm && sub ? sub.sql.slice(confirm.start - sub.base, confirm.end - sub.base) : ''}
        reason={confirm?.reason ?? ''}
        onConfirm={() => void runner.confirm()}
        onCancel={() => runner.dismissConfirm()}
      />
    </div>
  )
}
