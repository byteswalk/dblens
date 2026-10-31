// 比对标签（M4a 设计文档第 7 节）：上方两边的选择（模型或连接，再选 schema）、交换、选项；中间三层结果树
// （勾选框、只看差异、按状态过滤、疑似改名"按改名处理"）；右侧选中表的两边定义对照，不同的行标色；
// 底部依赖提示、"同步到目标"，目标是库时另有复制脚本、在查询标签中打开、另存为。由工作区懒加载。

import { IconArrowsExchange, IconCopy, IconDeviceFloppy, IconFileCode, IconPlayerPlay, IconRefresh } from '@tabler/icons-react'
import { useQuery, useQueryClient } from '@tanstack/react-query'
import { Alert, App, Button, Checkbox, Empty, Select, Space, Spin, Tag, Tree } from 'antd'
import type { DataNode } from 'antd/es/tree'
import { useMemo, useState } from 'react'
import { useTranslation } from 'react-i18next'

import { useApi } from '../../app/context'
import { useUi } from '../../app/ui'
import type {
  CompareOptions,
  CompareRenames,
  CompareRequest,
  CompareResult,
  CompareSideRef,
  Profile,
  SyncRequest,
  TableStatus,
} from '../../shared/api'
import { schemaOptions } from '../../shared/schemaOptions'
import { desktop } from '../../shared/shell'
import { withVault } from '../connections/vault'
import { useQueryActions } from '../editor/actions'
import { type CompareSide, type QueryTab, useTabs } from '../editor/tabs'
import {
  EMPTY_RENAMES,
  STATUSES,
  acceptSuggestion,
  compareLines,
  defaultSelection,
  dependencyIssues,
  describeItem,
  isDangerous,
  tableLabel,
  tableText,
} from './logic'
import SyncDialog from './SyncDialog'

const DEFAULT_OPTIONS: CompareOptions = {
  ignore_comments: false,
  ignore_column_order: false,
  ignore_generated_names: true,
  case_insensitive: false,
}

const STATUS_COLOR: Record<TableStatus, string> = {
  added: 'green',
  removed: 'red',
  renamed: 'blue',
  changed: 'orange',
  same: 'default',
}

/** 一边的选择：模型（打开的模型）或连接，再选 schema。 */
function SidePicker({ label, value, profiles, onChange }: {
  label: string
  value: CompareSide | null
  profiles: Profile[]
  onChange(v: CompareSide | null): void
}) {
  const api = useApi()
  const { t } = useTranslation()
  // 已选的一边以它的种类为准（交换两边后也对）；还没选时用下拉框选的种类
  const [picked, setKind] = useState<'model' | 'database'>(value?.kind ?? 'database')
  const kind = value?.kind ?? picked
  const models = useQuery({ queryKey: ['models'], queryFn: () => api.models(), retry: false })
  const model = value?.kind === 'model' ? models.data?.open.find((m) => m.path === value.path) : undefined
  const { data: modelTables = [] } = useQuery({
    queryKey: ['model-tables', model?.id],
    queryFn: () => api.modelTables(model!.id),
    enabled: !!model,
  })
  const profile = value?.kind === 'database' ? profiles.find((p) => p.id === value.profile) : undefined
  const { data: dbSchemas = [] } = useQuery({
    queryKey: ['schema-options', profile?.id],
    queryFn: () => withVault(api, () => schemaOptions(api, profile!)),
    enabled: !!profile,
  })
  const schemas =
    value?.kind === 'model'
      ? [...new Set([...(model?.info.schemas ?? []), ...modelTables.map((x) => x.schema)])].map((s) => ({
          value: s,
          label: s,
        }))
      : dbSchemas
  return (
    <Space.Compact>
      <span className="compare-side-label">{label}</span>
      <Select
        size="small"
        style={{ width: 90 }}
        value={kind}
        onChange={(k) => {
          setKind(k)
          onChange(null)
        }}
        options={[
          { value: 'database', label: t('model.compare.database') },
          { value: 'model', label: t('model.compare.model') },
        ]}
      />
      {kind === 'model' ? (
        <Select
          size="small"
          style={{ width: 180 }}
          placeholder={t('model.compare.pickModel')}
          value={value?.kind === 'model' ? value.path : undefined}
          onChange={(path) => onChange({ kind: 'model', path, schema: '' })}
          options={(models.data?.open ?? []).map((m) => ({ value: m.path, label: m.info.name, title: m.path }))}
        />
      ) : (
        <Select
          size="small"
          style={{ width: 180 }}
          placeholder={t('model.compare.pickProfile')}
          value={value?.kind === 'database' ? value.profile : undefined}
          onChange={(p) => onChange({ kind: 'database', profile: p, schema: '' })}
          options={profiles.map((p) => ({ value: p.id, label: p.name }))}
        />
      )}
      <Select
        size="small"
        style={{ width: 160 }}
        showSearch
        placeholder="schema"
        disabled={!value}
        value={value?.schema || undefined}
        onChange={(schema) => value && onChange({ ...value, schema })}
        options={schemas}
      />
    </Space.Compact>
  )
}

export default function CompareView({ tab, profiles }: { tab: QueryTab; profiles: Profile[] }) {
  const api = useApi()
  const { t } = useTranslation()
  const { message } = App.useApp()
  const qc = useQueryClient()
  const { openSql } = useQueryActions()
  const target = tab.compare!
  const [options, setOptions] = useState(DEFAULT_OPTIONS)
  const [renames, setRenames] = useState<CompareRenames>(EMPTY_RENAMES)
  const [result, setResult] = useState<{ req: CompareRequest; r: CompareResult } | null>(null)
  const [selected, setSelected] = useState<Set<number>>(new Set())
  const [onlyDiff, setOnlyDiff] = useState(true)
  const [statuses, setStatuses] = useState<TableStatus[]>([])
  const [focus, setFocus] = useState<number | null>(null)
  const [busy, setBusy] = useState(false)
  const [syncing, setSyncing] = useState(false)

  const setSides = (source: CompareSide | null, tgt: CompareSide | null) => {
    useTabs.getState().setCompare(tab.id, { source, target: tgt })
    setResult(null)
    setRenames(EMPTY_RENAMES)
  }
  const ready = (s: CompareSide | null): s is CompareSide => !!s && !!s.schema
  const toRef = async (s: CompareSide): Promise<CompareSideRef> =>
    s.kind === 'model'
      ? { kind: 'model', model: (await api.openModel(s.path)).id, schema: s.schema }
      : { kind: 'database', profile: s.profile, schema: s.schema }

  const run = async (r: CompareRenames = renames, keepSelection = false) => {
    if (!ready(target.source) || !ready(target.target)) return
    setBusy(true)
    try {
      const req: CompareRequest = {
        source: await toRef(target.source),
        target: await toRef(target.target),
        options,
        renames: r,
      }
      const res = await withVault(api, () => api.compare(req))
      setResult({ req, r: res })
      if (!keepSelection) setSelected(new Set(defaultSelection(res.diff.tables)))
      setFocus(null)
    } catch (e) {
      message.error((e as Error).message)
    } finally {
      setBusy(false)
    }
  }

  const r = result?.r
  const issues = useMemo(() => (r ? dependencyIssues(r, selected) : []), [r, selected])
  const tree: DataNode[] = useMemo(() => {
    if (!r) return []
    return r.diff.tables
      .map((d, index) => ({ d, index }))
      .filter(({ d }) => (!onlyDiff || d.status !== 'same') && (!statuses.length || statuses.includes(d.status)))
      .map(({ d, index }) => ({
        key: `t:${index}`,
        checkable: d.items.length > 0,
        title: (
          <span>
            <Tag color={STATUS_COLOR[d.status]}>{t(`model.compare.status.${d.status}`)}</Tag>
            {tableLabel(d)}
          </span>
        ),
        children: d.items.map((i) => ({
          key: `i:${i.id}`,
          isLeaf: true,
          title: (
            <span className={isDangerous(i.change) && selected.has(i.id) ? 'err' : undefined}>{describeItem(i.change)}</span>
          ),
        })),
      }))
  }, [r, onlyDiff, statuses, selected, t])

  const targetProfile = target.target?.kind === 'database' ? profiles.find((p) => p.id === (target.target as { profile: string }).profile) : undefined
  const syncRequest: SyncRequest | null = result && { compare: result.req, selected: [...selected], fingerprint: result.r.fingerprint }

  const scriptText = async () => (await api.syncPreview(syncRequest!)).text
  const scriptAction = async (kind: 'copy' | 'open' | 'save') => {
    try {
      const text = await scriptText()
      if (kind === 'copy') {
        await navigator.clipboard.writeText(text)
        message.success(t('model.compare.copied'))
      } else if (kind === 'open') openSql(text, targetProfile?.id)
      else if (desktop) {
        const path = await desktop.saveDialog('sync.sql', [{ name: 'SQL', extensions: ['sql'] }])
        if (path) {
          await api.saveScript(path, text)
          message.success(t('model.compare.saved'))
        }
      } else {
        const a = document.createElement('a')
        a.href = URL.createObjectURL(new Blob([text], { type: 'text/plain' }))
        a.download = 'sync.sql'
        a.click()
        URL.revokeObjectURL(a.href)
      }
    } catch (e) {
      message.error((e as Error).message)
    }
  }

  const afterSync = () => {
    setSyncing(false)
    if (targetProfile) useUi.getState().refreshTree(targetProfile.id, target.target!.schema)
    void qc.invalidateQueries({ queryKey: ['model-tables'] })
    void run()
  }

  const focused = focus === null ? undefined : r?.diff.tables[focus]
  const [left, right] = compareLines(
    tableText(r?.source_tables.find((x) => x.name === focused?.source)),
    tableText(r?.target_tables.find((x) => x.name === focused?.target)),
  )
  const option = (key: keyof CompareOptions) => (
    <Checkbox checked={options[key]} onChange={(e) => setOptions({ ...options, [key]: e.target.checked })}>
      {t(`model.compare.option.${key}`)}
    </Checkbox>
  )
  return (
    <div className="compare-view">
      <div className="row toolbar" style={{ flexWrap: 'wrap', gap: 8 }}>
        <SidePicker key={`s-${tab.id}`} label={t('model.compare.source')} value={target.source} profiles={profiles} onChange={(s) => setSides(s, target.target)} />
        <Button size="small" type="text" title={t('model.compare.swap')} icon={<IconArrowsExchange size={16} />} onClick={() => setSides(target.target, target.source)} />
        <SidePicker key={`t-${tab.id}`} label={t('model.compare.target')} value={target.target} profiles={profiles} onChange={(s) => setSides(target.source, s)} />
        <Button size="small" type="primary" icon={<IconRefresh size={16} />} loading={busy} disabled={!ready(target.source) || !ready(target.target)} onClick={() => void run()}>
          {t('model.compare.run')}
        </Button>
      </div>
      <div className="row toolbar" style={{ gap: 12 }}>
        {option('ignore_comments')}
        {option('ignore_column_order')}
        {option('ignore_generated_names')}
        {option('case_insensitive')}
        <span className="grow" />
        <Checkbox checked={onlyDiff} onChange={(e) => setOnlyDiff(e.target.checked)}>{t('model.compare.onlyDiff')}</Checkbox>
        <Select
          size="small"
          mode="multiple"
          allowClear
          style={{ minWidth: 160 }}
          placeholder={t('model.compare.statusFilter')}
          value={statuses}
          onChange={setStatuses}
          options={STATUSES.map((s) => ({ value: s, label: t(`model.compare.status.${s}`) }))}
        />
      </div>
      {busy && !r ? (
        <Spin style={{ margin: 48 }} />
      ) : !r ? (
        <Empty style={{ margin: 48 }} description={t('model.compare.hint')} />
      ) : (
        <>
          {[...r.source_errors, ...r.target_errors].map((e) => (
            <Alert key={e.path} type="error" showIcon message={t('model.compare.fileError', { path: e.path, message: e.message })} />
          ))}
          {r.diff.suggestions.length > 0 && (
            <Alert
              type="info"
              showIcon
              message={t('model.compare.suggestions')}
              description={r.diff.suggestions.map((s) => (
                <div key={`${s.table}/${s.source}`} className="row" style={{ gap: 8 }}>
                  <span>
                    {s.table ? `${s.table}.` : ''}
                    {s.target} → {s.source}
                  </span>
                  <Button
                    size="small"
                    type="link"
                    onClick={() => {
                      const next = acceptSuggestion(renames, s)
                      setRenames(next)
                      void run(next)
                    }}
                  >
                    {t('model.compare.acceptRename')}
                  </Button>
                </div>
              ))}
            />
          )}
          <div className="compare-body grow">
            <div className="compare-tree">
              <div className="muted">{t('model.compare.summary', { changed: r.diff.tables.filter((d) => d.status !== 'same').length, total: r.diff.tables.length })}</div>
              <Tree
                checkable
                blockNode
                defaultExpandAll
                treeData={tree}
                checkedKeys={[...selected].map((id) => `i:${id}`)}
                onCheck={(keys) => {
                  const list = Array.isArray(keys) ? keys : keys.checked
                  setSelected(new Set(list.map(String).filter((k) => k.startsWith('i:')).map((k) => Number(k.slice(2)))))
                }}
                onSelect={(keys) => {
                  const k = String(keys[0] ?? '')
                  const id = k.startsWith('i:') ? Number(k.slice(2)) : null
                  const index = k.startsWith('t:') ? Number(k.slice(2)) : r.diff.tables.findIndex((d) => d.items.some((i) => i.id === id))
                  setFocus(index >= 0 ? index : null)
                }}
              />
            </div>
            <div className="compare-detail">
              {focused ? (
                <>
                  <div className="compare-col">
                    <b>{t('model.compare.source')}: {focused.source ?? '—'}</b>
                    {left.map((l, i) => (
                      <div key={i} className={l.diff ? 'compare-line diff' : 'compare-line'}>{l.text}</div>
                    ))}
                  </div>
                  <div className="compare-col">
                    <b>{t('model.compare.target')}: {focused.target ?? '—'}</b>
                    {right.map((l, i) => (
                      <div key={i} className={l.diff ? 'compare-line diff' : 'compare-line'}>{l.text}</div>
                    ))}
                  </div>
                </>
              ) : (
                <span className="muted">{t('model.compare.pickTable')}</span>
              )}
            </div>
          </div>
          {issues.map((x) => (
            <Alert key={x} type="warning" showIcon message={x} />
          ))}
          <div className="row design-run">
            <span>{t('model.compare.selected', { count: selected.size })}</span>
            <span className="grow" />
            {targetProfile && (
              <>
                <Button size="small" icon={<IconCopy size={16} />} disabled={!selected.size} onClick={() => void scriptAction('copy')}>
                  {t('model.compare.copy')}
                </Button>
                <Button size="small" icon={<IconFileCode size={16} />} disabled={!selected.size} onClick={() => void scriptAction('open')}>
                  {t('model.compare.openQuery')}
                </Button>
                <Button size="small" icon={<IconDeviceFloppy size={16} />} disabled={!selected.size} onClick={() => void scriptAction('save')}>
                  {t('model.compare.saveAs')}
                </Button>
              </>
            )}
            <Button size="small" type="primary" icon={<IconPlayerPlay size={16} />} disabled={!selected.size} onClick={() => setSyncing(true)}>
              {t('model.compare.sync')}
            </Button>
          </div>
        </>
      )}
      {syncing && syncRequest && (
        <SyncDialog
          request={syncRequest}
          driver={targetProfile?.driver}
          profileId={targetProfile?.id ?? null}
          onDone={afterSync}
          onClose={() => {
            setSyncing(false)
            void run(renames, true)
          }}
        />
      )}
    </div>
  )
}
