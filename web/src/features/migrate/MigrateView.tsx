// 迁移标签（M5a 设计文档第 5 节）：上方选择源和目标、选项、打开/另存方案；读取结构后左侧对象列表（带提示、只看有问题的），
// 中间选中表的列映射表格（改目标名、类型、默认值，不迁某列，不认识的类型确认按文本迁），右侧这张表的目标 DDL；
// 底部同名表的处理方式、不能开始的原因、汇总、另存脚本、开始迁移，执行中显示进度，完成后可取迁移报告。由工作区懒加载。
// 数据迁移（M5b）：模式和数据选项、每张表的过滤条件和"只迁结构"、按表的进度、标签顶部的未完成的迁移。

import { IconDeviceFloppy, IconFolderOpen, IconPlayerPlay, IconRefresh, IconSettings } from '@tabler/icons-react'
import { keepPreviousData, useQuery, useQueryClient } from '@tanstack/react-query'
import { Alert, App, Button, Checkbox, Empty, Input, InputNumber, Modal, Progress, Select, Space, Spin, Table, Tag } from 'antd'
import { useEffect, useMemo, useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'

import { useApi } from '../../app/context'
import { useUi } from '../../app/ui'
import {
  type ExistingAction,
  type MigrationEnd,
  type MigrationMode,
  type MigrationNote,
  type MigrationPlan,
  type NameCase,
  type NoteLevel,
  type Profile,
  type TaskStatus,
  errorText,
  supportsWrites,
} from '../../shared/api'
import { desktop } from '../../shared/shell'
import { withVault } from '../connections/vault'
import SqlBox from '../design/SqlBox'
import { type QueryTab, useTabs } from '../editor/tabs'
import { ProgressTable, UnfinishedList } from './progress'
import {
  existingActions,
  notesOf,
  parsePlan,
  ready,
  setCheck,
  setColumn,
  setExisting,
  setSequence,
  setTable,
  statementsOf,
  summary,
  unchosen,
  worst,
} from './plan'

const LEVEL_COLOR: Record<NoteLevel, string> = { info: 'default', lossy: 'orange', unsupported: 'red' }

/** 一端的选择：连接，再选 schema。 */
function EndPicker({ label, value, profiles, onChange }: {
  label: string
  value: MigrationEnd
  profiles: Profile[]
  onChange(v: MigrationEnd): void
}) {
  const api = useApi()
  const { t } = useTranslation()
  const { data: schemas = [] } = useQuery({
    queryKey: ['schemas', value.profile],
    queryFn: () => withVault(api, () => api.schemas(value.profile)),
    enabled: !!value.profile,
  })
  return (
    <Space.Compact>
      <span className="compare-side-label">{label}</span>
      <Select
        size="small"
        style={{ width: 180 }}
        placeholder={t('migrate.pickProfile')}
        value={value.profile || undefined}
        onChange={(profile) => onChange({ profile, schema: '' })}
        options={profiles.filter((p) => supportsWrites(p.driver)).map((p) => ({ value: p.id, label: p.name }))}
      />
      <Select
        size="small"
        style={{ width: 160 }}
        showSearch
        placeholder="schema"
        disabled={!value.profile}
        value={value.schema || undefined}
        onChange={(schema) => onChange({ ...value, schema })}
        options={schemas.map((s) => ({ value: s.name }))}
      />
    </Space.Compact>
  )
}

/** 全局类型规则的编辑框。 */
function RulesDialog({ rules, onSave, onClose }: {
  rules: MigrationPlan['settings']['type_rules']
  onSave(r: MigrationPlan['settings']['type_rules']): void
  onClose(): void
}) {
  const { t } = useTranslation()
  const [list, setList] = useState(rules.length ? rules : [{ source: '', target: '' }])
  const edit = (i: number, k: 'source' | 'target', v: string) => setList(list.map((r, j) => (j === i ? { ...r, [k]: v } : r)))
  return (
    <Modal
      open
      title={t('migrate.rulesTitle')}
      onCancel={onClose}
      onOk={() => onSave(list.filter((r) => r.source.trim() && r.target.trim()))}
    >
      <div className="muted" style={{ marginBottom: 8 }}>{t('migrate.rulesHint')}</div>
      {list.map((r, i) => (
        <Space.Compact key={i} style={{ display: 'flex', marginBottom: 4 }}>
          <Input size="small" placeholder={t('migrate.ruleSource')} value={r.source} onChange={(e) => edit(i, 'source', e.target.value)} />
          <Input size="small" placeholder={t('migrate.ruleTarget')} value={r.target} onChange={(e) => edit(i, 'target', e.target.value)} />
          <Button size="small" onClick={() => setList(list.filter((_, j) => j !== i))}>×</Button>
        </Space.Compact>
      ))}
      <Button size="small" type="dashed" onClick={() => setList([...list, { source: '', target: '' }])}>
        {t('migrate.addRule')}
      </Button>
    </Modal>
  )
}

/** 失去焦点或回车时才提交的输入框（避免每个字都重新预览）。 */
function LazyInput({ value, placeholder, onCommit }: { value: string | undefined; placeholder: string; onCommit(v: string | undefined): void }) {
  const [text, setText] = useState(value ?? '')
  useEffect(() => setText(value ?? ''), [value])
  const commit = () => {
    const v = text.trim() ? text : undefined
    if (v !== value) onCommit(v)
  }
  return <Input size="small" style={{ minWidth: 120 }} value={text} placeholder={placeholder} onChange={(e) => setText(e.target.value)} onBlur={commit} onPressEnter={commit} />
}

function NoteTags({ notes }: { notes: MigrationNote[] }) {
  const { t } = useTranslation()
  return (
    <>
      {notes.map((n, i) => (
        <div key={i}>
          <Tag color={LEVEL_COLOR[n.level]}>{t(`migrate.level.${n.level}`)}</Tag>
          {n.item ? `[${n.item}] ` : ''}
          {n.message}
        </div>
      ))}
    </>
  )
}

function download(name: string, text: string, type = 'text/plain') {
  const a = document.createElement('a')
  a.href = URL.createObjectURL(new Blob([text], { type }))
  a.download = name
  a.click()
  URL.revokeObjectURL(a.href)
}

export default function MigrateView({ tab, profiles }: { tab: QueryTab; profiles: Profile[] }) {
  const api = useApi()
  const { t } = useTranslation()
  const { message, modal } = App.useApp()
  const plan = tab.migrate!
  const setPlan = (p: MigrationPlan) => useTabs.getState().setMigrate(tab.id, p)
  const [loaded, setLoaded] = useState(false)
  const [focus, setFocus] = useState<{ kind: 'table' | 'sequence'; name: string } | null>(null)
  const [onlyIssues, setOnlyIssues] = useState(false)
  const [rules, setRules] = useState(false)
  const [task, setTask] = useState<TaskStatus | null>(null)
  const [busy, setBusy] = useState(false)
  const file = useRef<HTMLInputElement>(null)
  const qc = useQueryClient()
  const track = (id: string) => setTask({ id, state: 'running', progress: { statements_done: 0, statements_total: 0 }, failures: [], migration: null } as unknown as TaskStatus)

  const { data: p, error, isFetching, refetch } = useQuery({
    queryKey: ['migration-preview', JSON.stringify(plan)],
    queryFn: () => withVault(api, () => api.previewMigration(plan)),
    enabled: loaded && ready(plan),
    placeholderData: keepPreviousData,
    retry: false,
  })

  const setEnds = (source: MigrationEnd, target: MigrationEnd) => {
    setPlan({ ...plan, source, target, tables: null, sequences: null, existing: {} })
    setLoaded(false)
    setFocus(null)
  }

  // 任务进度
  useEffect(() => {
    if (!task || task.state !== 'running') return
    const h = setTimeout(async () => {
      try {
        const st = await api.task(task.id)
        setTask(st)
        if (st.state !== 'running') {
          useUi.getState().refreshTree(plan.target.profile, plan.target.schema)
          void qc.invalidateQueries({ queryKey: ['migrations'] })
          if (loaded) void refetch()
        }
      } catch (e) {
        message.error((e as Error).message)
        setTask(null)
      }
    }, 500)
    return () => clearTimeout(h)
  }, [task, api, message, plan.target, refetch, qc, loaded])

  const target = profiles.find((x) => x.id === plan.target.profile)
  const sum = p && summary(p)
  const pending = p ? unchosen(plan, p) : []
  const running = task?.state === 'running'

  const start = async () => {
    setBusy(true)
    try {
      track(await withVault(api, () => api.startMigration(plan)))
    } catch (e) {
      message.error((e as Error).message)
    } finally {
      setBusy(false)
    }
  }
  const confirmStart = () => {
    if (!p?.confirm.length) return void start()
    modal.confirm({
      title: t('migrate.confirmTitle'),
      content: p.confirm.map((c) => <div key={c}>{c}</div>),
      okButtonProps: { danger: true },
      onOk: () => start(),
    })
  }
  const saveScript = async () => {
    if (!p) return
    try {
      if (desktop) {
        const path = await desktop.saveDialog('migration.sql', [{ name: 'SQL', extensions: ['sql'] }])
        if (!path) return
        await api.saveScript(path, p.text)
        message.success(t('migrate.saved'))
      } else download('migration.sql', p.text)
    } catch (e) {
      message.error((e as Error).message)
    }
  }
  const saveReport = async () => {
    if (!task?.report) return
    try {
      if (desktop) {
        const path = await desktop.saveDialog(task.report, [{ name: 'Markdown', extensions: ['md'] }])
        if (!path) return
        await api.saveTaskReport(task.id, path)
        message.success(t('migrate.saved'))
      } else {
        const a = document.createElement('a')
        a.href = api.taskReportUrl(task.id)
        a.download = task.report
        a.click()
      }
    } catch (e) {
      message.error((e as Error).message)
    }
  }
  const openPlan = async (f: File | undefined) => {
    if (!f) return
    const next = parsePlan(await f.text())
    if (!next) return void message.error(t('migrate.badPlan'))
    setPlan(next)
    setLoaded(false)
    setFocus(null)
  }

  // 左侧对象列表
  const objects = useMemo(() => {
    if (!p) return []
    const tables = p.source_tables.map((x) => {
      const m = p.draft.tables.find((d) => d.source === x.name)
      return {
        kind: 'table' as const,
        name: x.name,
        target: m?.target.name,
        exists: !!m?.exists,
        skip: !!plan.settings.tables[x.name]?.skip,
        level: worst(notesOf(p, 'table', x.name)),
      }
    })
    const seqs = p.source_sequences.map((x) => {
      const m = p.draft.sequences.find((d) => d.source === x.name)
      return {
        kind: 'sequence' as const,
        name: x.name,
        target: m?.target.name,
        exists: !!m?.exists,
        skip: !!plan.settings.sequences[x.name]?.skip,
        level: worst(notesOf(p, 'sequence', x.name)),
      }
    })
    return [...tables, ...seqs].filter((o) => !onlyIssues || (o.level && o.level !== 'info'))
  }, [p, plan.settings, onlyIssues])

  const source = focus?.kind === 'table' ? p?.source_tables.find((x) => x.name === focus.name) : undefined
  const mapped = focus ? p?.draft.tables.find((d) => d.source === focus.name) : undefined
  const tov = focus ? plan.settings.tables[focus.name] : undefined
  const data = plan.mode !== 'structure'
  const setData = (patch: Partial<MigrationPlan['data']>) => setPlan({ ...plan, data: { ...plan.data, ...patch } })
  const rows = (source?.columns ?? []).map((c) => {
    const target = mapped?.columns.find((m) => m.source === c.name)?.target
    return { c, ov: tov?.columns?.[c.name] ?? {}, tc: mapped?.target.columns.find((x) => x.name === target), notes: p ? notesOf(p, 'table', focus!.name, c.name) : [] }
  })

  return (
    <div className="compare-view">
      <UnfinishedList onResume={track} />
      <div className="row toolbar" style={{ flexWrap: 'wrap', gap: 8 }}>
        <EndPicker label={t('migrate.source')} value={plan.source} profiles={profiles} onChange={(s) => setEnds(s, plan.target)} />
        <span>→</span>
        <EndPicker label={t('migrate.target')} value={plan.target} profiles={profiles} onChange={(x) => setEnds(plan.source, x)} />
        <Button
          size="small"
          type="primary"
          icon={<IconRefresh size={16} />}
          loading={isFetching}
          disabled={!ready(plan)}
          onClick={() => (loaded ? void refetch() : setLoaded(true))}
        >
          {t('migrate.read')}
        </Button>
        <span className="grow" />
        <Button size="small" icon={<IconFolderOpen size={16} />} onClick={() => file.current?.click()}>{t('migrate.openPlan')}</Button>
        <Button size="small" icon={<IconDeviceFloppy size={16} />} onClick={() => download('migration-plan.json', JSON.stringify(plan, null, 2), 'application/json')}>
          {t('migrate.savePlan')}
        </Button>
        <input ref={file} type="file" accept=".json" hidden onChange={(e) => void openPlan(e.target.files?.[0]).finally(() => (e.target.value = ''))} />
      </div>
      <div className="row toolbar" style={{ gap: 12 }}>
        <span>{t('migrate.nameCase.label')}</span>
        <Select
          size="small"
          style={{ width: 120 }}
          value={plan.settings.options.name_case ?? 'auto'}
          onChange={(v: NameCase | 'auto') => setPlan({ ...plan, settings: { ...plan.settings, options: { ...plan.settings.options, name_case: v === 'auto' ? null : v } } })}
          options={(['auto', 'keep', 'lower', 'upper'] as const).map((v) => ({ value: v, label: t(`migrate.nameCase.${v}`) }))}
        />
        {(['comments', 'checks'] as const).map((k) => (
          <Checkbox
            key={k}
            checked={plan.settings.options[k]}
            onChange={(e) => setPlan({ ...plan, settings: { ...plan.settings, options: { ...plan.settings.options, [k]: e.target.checked } } })}
          >
            {t(`migrate.${k}`)}
          </Checkbox>
        ))}
        <Button size="small" icon={<IconSettings size={16} />} onClick={() => setRules(true)}>
          {t('migrate.typeRules')}
          {plan.settings.type_rules.length ? ` (${plan.settings.type_rules.length})` : ''}
        </Button>
      </div>
      <div className="row toolbar" style={{ gap: 12, flexWrap: 'wrap' }}>
        <span>{t('migrate.mode.label')}</span>
        <Select
          size="small"
          style={{ width: 140 }}
          value={plan.mode}
          onChange={(mode: MigrationMode) => setPlan({ ...plan, mode, existing: {} })}
          options={(['all', 'structure', 'data'] as const).map((v) => ({ value: v, label: t(`migrate.mode.${v}`) }))}
        />
        {data && (
          <>
            <span>{t('migrate.data.batch')}</span>
            <InputNumber size="small" min={100} max={100000} step={500} value={plan.data.batch_size} onChange={(v) => v && setData({ batch_size: v })} />
            <span>{t('migrate.data.parallel')}</span>
            <InputNumber size="small" min={1} max={4} value={plan.data.parallel} onChange={(v) => v && setData({ parallel: v })} />
            <span>{t('migrate.data.onError')}</span>
            <Select
              size="small"
              style={{ width: 130 }}
              value={plan.data.on_error}
              onChange={(on_error: 'stop' | 'skip') => setData({ on_error })}
              options={(['stop', 'skip'] as const).map((v) => ({ value: v, label: t(`migrate.data.${v}`) }))}
            />
            <span title={t('migrate.data.sampleHint')}>{t('migrate.data.sample')}</span>
            <InputNumber size="small" min={0} max={10000} step={100} value={plan.data.sample} onChange={(v) => setData({ sample: v ?? 0 })} />
          </>
        )}
      </div>
      {task?.migration && <ProgressTable progress={task.migration} />}
      {error ? (
        <Alert type="error" showIcon message={(error as Error).message} />
      ) : !p ? (
        isFetching ? <Spin style={{ margin: 48 }} /> : <Empty style={{ margin: 48 }} description={t('migrate.hint')} />
      ) : (
        <>
          <div className="compare-body grow">
            <div className="migrate-objects">
              <Checkbox checked={onlyIssues} onChange={(e) => setOnlyIssues(e.target.checked)}>{t('migrate.onlyIssues')}</Checkbox>
              {objects.map((o) => (
                <div
                  key={`${o.kind}:${o.name}`}
                  className={`migrate-object${focus?.kind === o.kind && focus.name === o.name ? ' on' : ''}`}
                  onClick={() => setFocus({ kind: o.kind, name: o.name })}
                >
                  <Checkbox
                    checked={!o.skip}
                    onClick={(e) => e.stopPropagation()}
                    onChange={(e) =>
                      setPlan(o.kind === 'table' ? setTable(plan, o.name, { skip: !e.target.checked }) : setSequence(plan, o.name, { skip: !e.target.checked }))
                    }
                  />
                  <span className="grow">
                    {o.kind === 'sequence' && <span className="muted">{t('migrate.sequences')} · </span>}
                    {o.name}
                    {o.target && o.target !== o.name && <span className="muted"> → {o.target}</span>}
                  </span>
                  {o.exists && <Tag>{t('migrate.exists')}</Tag>}
                  {o.level && o.level !== 'info' && <Tag color={LEVEL_COLOR[o.level]}>{t(`migrate.level.${o.level}`)}</Tag>}
                </div>
              ))}
              {p.others.length > 0 && (
                <details className="muted">
                  <summary>{t('migrate.others', { count: p.others.length })}</summary>
                  <div>{t('migrate.othersHint')}</div>
                  {p.others.map((o) => (
                    <div key={`${o.kind}:${o.table ?? ''}:${o.name}`}>
                      {o.kind} {o.table ? `${o.table}.` : ''}
                      {o.name}
                    </div>
                  ))}
                </details>
              )}
            </div>
            <div className="migrate-mapping">
              {!focus ? (
                <span className="muted">{t('migrate.pickTable')}</span>
              ) : focus.kind === 'sequence' ? (
                <>
                  <Space>
                    {t('migrate.col.name')}
                    <LazyInput
                      value={plan.settings.sequences[focus.name]?.name}
                      placeholder={p.draft.sequences.find((s) => s.source === focus.name)?.target.name ?? focus.name}
                      onCommit={(name) => setPlan(setSequence(plan, focus.name, { name }))}
                    />
                  </Space>
                  <NoteTags notes={notesOf(p, 'sequence', focus.name)} />
                </>
              ) : (
                <>
                  <Space wrap>
                    {t('migrate.tableName')}
                    <LazyInput value={tov?.name} placeholder={mapped?.target.name ?? focus.name} onCommit={(name) => setPlan(setTable(plan, focus.name, { name }))} />
                    {data && (
                      <>
                        <LazyInput value={tov?.filter} placeholder={t('migrate.filter')} onCommit={(filter) => setPlan(setTable(plan, focus.name, { filter }))} />
                        <Checkbox
                          checked={!!tov?.structure_only}
                          onChange={(e) => setPlan(setTable(plan, focus.name, { structure_only: e.target.checked }))}
                        >
                          {t('migrate.structureOnly')}
                        </Checkbox>
                      </>
                    )}
                  </Space>
                  <NoteTags notes={notesOf(p, 'table', focus.name, null)} />
                  <Table
                    size="small"
                    pagination={false}
                    scroll={{ x: 'max-content' }}
                    rowKey={(r) => r.c.name}
                    dataSource={rows}
                    columns={[
                      {
                        title: t('migrate.col.migrate'),
                        width: 56,
                        render: (_, r) => (
                          <Checkbox checked={!r.ov.skip} onChange={(e) => setPlan(setColumn(plan, focus.name, r.c.name, { skip: !e.target.checked || undefined }))} />
                        ),
                      },
                      { title: t('migrate.col.source'), render: (_, r) => r.c.name },
                      { title: t('migrate.col.sourceType'), render: (_, r) => <code>{r.c.data_type.native}</code> },
                      {
                        title: t('migrate.col.name'),
                        render: (_, r) => (
                          <LazyInput value={r.ov.name} placeholder={r.tc?.name ?? r.c.name} onCommit={(name) => setPlan(setColumn(plan, focus.name, r.c.name, { name }))} />
                        ),
                      },
                      {
                        title: t('migrate.col.type'),
                        render: (_, r) => (
                          <LazyInput value={r.ov.type} placeholder={r.tc?.data_type.native ?? ''} onCommit={(type) => setPlan(setColumn(plan, focus.name, r.c.name, { type }))} />
                        ),
                      },
                      {
                        title: t('migrate.col.default'),
                        render: (_, r) =>
                          r.c.generated ? (
                            <LazyInput
                              value={r.ov.expression}
                              placeholder={`= ${r.tc?.generated ?? ''}`}
                              onCommit={(expression) => setPlan(setColumn(plan, focus.name, r.c.name, { expression }))}
                            />
                          ) : (
                            <LazyInput
                              value={r.ov.default}
                              placeholder={r.tc?.default ?? t('migrate.noDefault')}
                              onCommit={(d) => setPlan(setColumn(plan, focus.name, r.c.name, { default: d }))}
                            />
                          ),
                      },
                      {
                        title: t('migrate.col.notes'),
                        render: (_, r) => (
                          <>
                            <NoteTags notes={r.notes} />
                            {r.notes.some((n) => n.level === 'unsupported') && !r.ov.accept && (
                              <Button size="small" type="link" onClick={() => setPlan(setColumn(plan, focus.name, r.c.name, { accept: true }))}>
                                {t('migrate.acceptText')}
                              </Button>
                            )}
                          </>
                        ),
                      },
                    ]}
                  />
                  {(source?.checks ?? []).filter((c) => c.name).map((c) => (
                    <Space key={c.name} style={{ marginTop: 4 }}>
                      <code>CHECK {c.name}</code>
                      <LazyInput
                        value={tov?.checks?.[c.name!]}
                        placeholder={mapped?.target.checks.find((x) => x.name?.toLowerCase() === c.name!.toLowerCase())?.expression ?? c.expression}
                        onCommit={(e) => setPlan(setCheck(plan, focus.name, c.name!, e))}
                      />
                    </Space>
                  ))}
                </>
              )}
            </div>
            <div className="migrate-ddl">
              <b>{t('migrate.ddl')}</b>
              <SqlBox sql={focus ? statementsOf(p, focus.name) : p.text} driver={target?.driver} height="100%" />
            </div>
          </div>
          {p.draft.tables.some((x) => x.exists) && (
            <div className="row toolbar" style={{ flexWrap: 'wrap', gap: 8 }}>
              <b>{t('migrate.existing.title')}</b>
              <span className="muted">{t('migrate.existing.all')}</span>
              <Select
                size="small"
                style={{ width: 200 }}
                placeholder={t('migrate.existing.pick')}
                value={null}
                onChange={(a: ExistingAction) => setPlan(setExisting(plan, p.draft.tables.filter((x) => x.exists).map((x) => x.source), a))}
                options={existingActions(plan.mode).map((a) => ({ value: a, label: t(`migrate.existing.${a}`) }))}
              />
              {p.draft.tables
                .filter((x) => x.exists)
                .map((x) => (
                  <Space.Compact key={x.source}>
                    <span className="compare-side-label">{x.target.name}</span>
                    <Select
                      size="small"
                      style={{ width: 200 }}
                      status={plan.existing[x.source] ? undefined : 'warning'}
                      placeholder={t('migrate.existing.pick')}
                      value={plan.existing[x.source]}
                      onChange={(a: ExistingAction) => setPlan(setExisting(plan, [x.source], a))}
                      options={existingActions(plan.mode).map((a) => ({ value: a, label: t(`migrate.existing.${a}`) }))}
                    />
                  </Space.Compact>
                ))}
            </div>
          )}
          {p.denied && <Alert type="error" showIcon message={p.denied} />}
          {p.problems.map((x) => (
            <Alert key={x} type="warning" showIcon message={x} />
          ))}
          {task && !running && (
            <Alert
              type={task.state === 'done' && !task.failures.length ? 'success' : task.state === 'done' || task.state === 'cancelled' ? 'warning' : 'error'}
              showIcon
              message={
                task.state === 'done'
                  ? task.migration?.tables.some((x) => x.state === 'failed')
                    ? t('migrate.unfinishedDone')
                    : task.failures.length
                      ? t('migrate.doneWithFailures', { count: task.progress.statements_failed })
                      : t('migrate.done')
                  : t(task.state === 'cancelled' ? 'migrate.cancelled' : 'migrate.failed')
              }
              description={
                <>
                  {task.failures.slice(0, 5).map((f) => (
                    <div key={f.index}>
                      <code className="design-failed-sql">{f.text}</code>
                      <div className="err">{errorText(f.error)}</div>
                    </div>
                  ))}
                  {task.error && <div className="err">{errorText(task.error)}</div>}
                </>
              }
              action={
                task.report ? (
                  <Button size="small" onClick={() => void saveReport()}>
                    {t('migrate.report')}
                  </Button>
                ) : undefined
              }
            />
          )}
          <div className="row design-run">
            {running ? (
              <>
                <span>{t('migrate.running', { done: task.progress.statements_done, total: task.progress.statements_total })}</span>
                <Progress size="small" style={{ width: 240 }} percent={Math.round((100 * task.progress.statements_done) / Math.max(1, task.progress.statements_total))} />
                <Button size="small" onClick={() => void api.cancelTask(task.id).catch((e) => message.error((e as Error).message))}>
                  {t('migrate.cancel')}
                </Button>
              </>
            ) : (
              sum && <span>{t('migrate.summary', sum)}</span>
            )}
            <span className="grow" />
            {data && <span className="muted">{t('migrate.sourceHint')}</span>}
            <Button size="small" icon={<IconDeviceFloppy size={16} />} disabled={!p.statements.length} onClick={() => void saveScript()}>
              {t('migrate.saveScript')}
            </Button>
            <Button
              size="small"
              type="primary"
              danger={!!p.confirm.length}
              icon={<IconPlayerPlay size={16} />}
              loading={busy}
              disabled={running || isFetching || !!p.problems.length || !!p.denied || pending.length > 0 || (!data && !p.statements.length)}
              onClick={confirmStart}
            >
              {t('migrate.start')}
            </Button>
          </div>
        </>
      )}
      {rules && (
        <RulesDialog
          rules={plan.settings.type_rules}
          onClose={() => setRules(false)}
          onSave={(type_rules) => {
            setPlan({ ...plan, settings: { ...plan.settings, type_rules } })
            setRules(false)
          }}
        />
      )}
    </div>
  )
}
