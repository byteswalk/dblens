// 一个查询标签页的内容：工具栏、提示条、编辑器和结果面板（可拖动分隔线，位置存进设置）、确认框。
// 处理编辑器命令：执行当前语句（有选中时执行选中部分）、执行全部、停止、格式化、执行计划。

import { Alert, App, Button } from 'antd'
import { useCallback, useEffect, useMemo, useRef } from 'react'
import { useTranslation } from 'react-i18next'
import { Group, Panel, Separator, useDefaultLayout } from 'react-resizable-panels'

import { useApi } from '../../app/context'
import { type Profile, errorText } from '../../shared/api'
import { layoutStorage } from '../../shared/settings'
import ResultsPanel from '../results/ResultsPanel'
import ConfirmDialog from './ConfirmDialog'
import Toolbar from './Toolbar'
import { onCommand } from './commands'
import { editorLanguage, formatSql } from './dialect'
import { errorMarks } from './markers'
import { runnerFor, useTabRuntime } from './runner'
import SqlEditor, { type MonacoEditor } from './SqlEditor'
import { type QueryTab, useTabs } from './tabs'

export default function QueryView({ tab, profiles }: { tab: QueryTab; profiles: Profile[] }) {
  const api = useApi()
  const { t } = useTranslation()
  const { message } = App.useApp()
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
    [runner, profile, rt.session?.variant, message, t],
  )

  // 协议层面的错误（如上一段还没执行完）用全局提示（M1 设计文档第 5 节），同时记在"消息"里
  const protocolError = rt.exec.protocolError
  useEffect(() => {
    if (protocolError) message.error(errorText(protocolError))
  }, [protocolError, message])

  const marks = useMemo(() => errorMarks(rt.exec, rt.source), [rt.exec, rt.source])
  const confirm = !rt.running && !rt.declined ? rt.exec.confirm : null
  const sub = rt.submission

  return (
    <div className="query">
      <Toolbar tab={tab} rt={rt} runner={runner} profiles={profiles} />
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
            language={profile ? editorLanguage(profile.driver) : 'sql'}
            defaultValue={tab.sql}
            marks={marks}
            onChange={(v) => useTabs.getState().setSql(tab.id, v)}
            onReady={onReady}
          />
        </Panel>
        <Separator className="splitter-h" />
        <Panel id="results" defaultSize="40%" minSize={60}>
          <ResultsPanel rt={rt} onFetchMore={() => runner.fetchMore()} />
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
