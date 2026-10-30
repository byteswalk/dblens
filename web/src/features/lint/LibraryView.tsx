// 规范库：左侧规范列表，右侧条款表。可改开关、级别、参数，复制、删除、恢复默认、导入导出。

import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { App, Button, Empty, Input, InputNumber, Select, Space, Spin, Switch, Table, Tag } from 'antd'
import { useEffect, useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'

import { useApi } from '../../app/context'
import type { LintClause, LintException, LintRuleInfo, LintRuleset, LintSeverity } from '../../shared/api'
import { SECTION_ORDER, download } from './logic'

const SEVERITIES: LintSeverity[] = ['error', 'warning', 'info']

export default function LibraryView() {
  const api = useApi()
  const { t } = useTranslation()
  const { message, modal } = App.useApp()
  const qc = useQueryClient()
  const [selected, setSelected] = useState<string | undefined>(undefined)

  const list = useQuery({ queryKey: ['lint', 'rulesets'], queryFn: () => api.lintRulesets() })
  const rules = useQuery({ queryKey: ['lint', 'rules'], queryFn: () => api.lintRules() })

  const current = selected ?? list.data?.[0]?.id
  const summary = list.data?.find((r) => r.id === current)

  const refresh = () => qc.invalidateQueries({ queryKey: ['lint', 'rulesets'] })

  const create = useMutation({
    mutationFn: (set: LintRuleset) => api.createLintRuleset(set),
    onSuccess: (id) => {
      void refresh()
      setSelected(id)
    },
    onError: (e) => message.error(e instanceof Error ? e.message : String(e)),
  })

  const importFile = useRef<HTMLInputElement>(null)
  async function onImport(file: File) {
    try {
      const id = await api.importLintRuleset(await file.text())
      void refresh()
      setSelected(id)
      message.success(t('lint.lib.imported'))
    } catch (e) {
      message.error(e instanceof Error ? e.message : String(e))
    }
  }

  return (
    <div style={{ display: 'flex', height: '100%' }}>
      <div style={{ width: 240, borderRight: '1px solid var(--border, #e3e3e3)', display: 'flex', flexDirection: 'column' }}>
        <div style={{ padding: 8, display: 'flex', gap: 6 }}>
          <Button
            size="small"
            onClick={() =>
              create.mutate({ id: '', name: t('lint.lib.new_name'), builtin: false, clauses: [] })
            }
          >
            {t('lint.lib.new')}
          </Button>
          <Button size="small" onClick={() => importFile.current?.click()}>
            {t('lint.lib.import')}
          </Button>
          <input
            ref={importFile}
            type="file"
            accept=".yaml,.yml"
            style={{ display: 'none' }}
            onChange={(e) => {
              const f = e.target.files?.[0]
              if (f) void onImport(f)
              e.target.value = ''
            }}
          />
        </div>
        {list.isLoading ? (
          <Spin style={{ margin: 24 }} />
        ) : (
          <div style={{ overflow: 'auto', flex: 1 }}>
            {list.data?.map((r) => (
              <div
                key={r.id}
                onClick={() => setSelected(r.id)}
                style={{
                  padding: '6px 10px',
                  cursor: 'pointer',
                  background: r.id === current ? 'var(--hover, #f0f0f0)' : undefined,
                  borderLeft: r.id === current ? '2px solid #555' : '2px solid transparent',
                }}
              >
                <div>{r.name}</div>
                <div style={{ fontSize: 11, color: '#999' }}>
                  {r.builtin ? t('lint.builtin') : t('lint.custom')}
                  {r.modified ? ` · ${t('lint.lib.modified')}` : ''} · {r.clause_count}
                </div>
              </div>
            ))}
          </div>
        )}
      </div>

      <div style={{ flex: 1, overflow: 'hidden' }}>
        {current && summary ? (
          <Editor
            key={current}
            id={current}
            builtin={summary.builtin}
            rules={rules.data ?? []}
            onCopied={(id) => {
              void refresh()
              setSelected(id)
            }}
            onDeleted={() => {
              void refresh()
              setSelected(undefined)
            }}
            onChangedList={refresh}
            confirm={modal.confirm}
          />
        ) : (
          <Empty style={{ marginTop: 48 }} description={t('lint.lib.empty')} />
        )}
      </div>
    </div>
  )
}

function Editor({
  id,
  builtin,
  rules,
  onCopied,
  onDeleted,
  onChangedList,
  confirm,
}: {
  id: string
  builtin: boolean
  rules: LintRuleInfo[]
  onCopied: (id: string) => void
  onDeleted: () => void
  onChangedList: () => void
  confirm: ReturnType<typeof App.useApp>['modal']['confirm']
}) {
  const api = useApi()
  const { t } = useTranslation()
  const { message } = App.useApp()
  const { data, isLoading } = useQuery({ queryKey: ['lint', 'ruleset', id], queryFn: () => api.lintRuleset(id) })
  const [draft, setDraft] = useState<LintRuleset | null>(null)
  const [dirty, setDirty] = useState(false)

  useEffect(() => {
    if (!data) return
    // 结果页豁免后规范会刷新：有未保存的改动时只带上例外，不冲掉改动
    setDraft((d) => (d && dirty ? { ...d, exceptions: data.exceptions } : structuredClone(data)))
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [data])

  if (isLoading || !draft) return <Spin style={{ margin: 48 }} />

  const edit = (next: LintRuleset) => {
    setDraft(next)
    setDirty(true)
  }
  const patchClause = (cid: string, patch: Partial<LintClause>) =>
    edit({ ...draft, clauses: draft.clauses.map((c) => (c.id === cid ? { ...c, ...patch } : c)) })

  const save = async () => {
    try {
      await api.updateLintRuleset(id, draft)
      setDirty(false)
      onChangedList()
      message.success(t('lint.lib.saved'))
    } catch (e) {
      message.error(e instanceof Error ? e.message : String(e))
    }
  }

  const copy = async () => {
    try {
      const copyName = t('lint.lib.copy_name', { name: draft.name })
      const newId = await api.createLintRuleset({ ...draft, id: '', builtin: false, name: copyName })
      onCopied(newId)
    } catch (e) {
      message.error(e instanceof Error ? e.message : String(e))
    }
  }

  const exceptions = draft.exceptions ?? []
  const removeException = (x: LintException) =>
    edit({ ...draft, exceptions: exceptions.filter((e) => e !== x) })
  const removeClause = (cid: string) =>
    edit({
      ...draft,
      clauses: draft.clauses.filter((c) => c.id !== cid),
      exceptions: exceptions.filter((e) => e.clause !== cid),
    })

  const groups = SECTION_ORDER.map((section) => ({
    section,
    clauses: draft.clauses.filter((c) => c.section === section),
  })).filter((g) => g.clauses.length)

  return (
    <div style={{ height: '100%', display: 'flex', flexDirection: 'column' }}>
      <div style={{ padding: 8, display: 'flex', gap: 6, alignItems: 'center', borderBottom: '1px solid var(--border, #e3e3e3)' }}>
        <Input
          size="small"
          style={{ width: 200 }}
          value={draft.name}
          disabled={builtin}
          onChange={(e) => edit({ ...draft, name: e.target.value })}
        />
        <Button type="primary" size="small" disabled={!dirty} onClick={() => void save()}>
          {t('lint.lib.save')}
        </Button>
        <Button size="small" onClick={() => void copy()}>
          {t('lint.lib.copy')}
        </Button>
        <Button size="small" onClick={() => download(api.lintRulesetExportUrl(id))}>
          {t('lint.lib.export')}
        </Button>
        {builtin ? (
          <Button
            size="small"
            onClick={() =>
              confirm({
                title: t('lint.lib.reset_confirm'),
                onOk: async () => {
                  await api.resetLintRuleset(id)
                  onChangedList()
                  setDirty(false)
                  const fresh = await api.lintRuleset(id)
                  setDraft(structuredClone(fresh))
                },
              })
            }
          >
            {t('lint.lib.reset')}
          </Button>
        ) : (
          <Button
            size="small"
            danger
            onClick={() =>
              confirm({
                title: t('lint.lib.delete_confirm'),
                onOk: async () => {
                  await api.deleteLintRuleset(id)
                  onDeleted()
                },
              })
            }
          >
            {t('lint.lib.delete')}
          </Button>
        )}
        <span style={{ flex: 1 }} />
        {!builtin && (
          <Button size="small" onClick={() => edit({ ...draft, clauses: [...draft.clauses, newManualClause(draft)] })}>
            {t('lint.lib.add_clause')}
          </Button>
        )}
      </div>

      <div style={{ overflow: 'auto', flex: 1, padding: '4px 12px' }}>
        {groups.map((g) => (
          <div key={g.section}>
            <div style={{ fontWeight: 600, margin: '10px 0 4px' }}>{t(`lint.section.${g.section}`)}</div>
            <Table<LintClause>
              size="small"
              pagination={false}
              rowKey="id"
              dataSource={g.clauses}
              expandable={{
                rowExpandable: (c) => (c.rules?.length ?? 0) > 0,
                expandedRowRender: (c) => (
                  <Params clause={c} rules={rules} onChange={(rs) => patchClause(c.id, { rules: rs })} />
                ),
              }}
              columns={[
                {
                  title: '',
                  width: 48,
                  render: (_: unknown, c) => (
                    <Switch size="small" checked={c.enabled} onChange={(v) => patchClause(c.id, { enabled: v })} />
                  ),
                },
                {
                  title: t('lint.lib.clause'),
                  render: (_: unknown, c) =>
                    builtin ? (
                      c.text
                    ) : (
                      <Input
                        size="small"
                        variant="borderless"
                        value={c.text}
                        onChange={(e) => patchClause(c.id, { text: e.target.value })}
                      />
                    ),
                },
                {
                  title: t('lint.lib.severity'),
                  width: 110,
                  render: (_: unknown, c) => (
                    <Select
                      size="small"
                      style={{ width: 90 }}
                      value={c.severity}
                      onChange={(v) => patchClause(c.id, { severity: v })}
                      options={SEVERITIES.map((v) => ({ value: v, label: t(`lint.severity.${v}`) }))}
                    />
                  ),
                },
                {
                  title: t('lint.lib.rules'),
                  width: 70,
                  render: (_: unknown, c) => (c.rules?.length ? <Tag>{c.rules.length}</Tag> : <span style={{ color: '#bbb' }}>{t('lint.lib.manual')}</span>),
                },
                // 内置规范只能用开关关闭：差异按条款记，恢复默认要能还原
                ...(builtin
                  ? []
                  : [
                      {
                        title: '',
                        width: 70,
                        render: (_: unknown, c: LintClause) => (
                          <Button type="link" size="small" onClick={() => removeClause(c.id)}>
                            {t('lint.lib.delete')}
                          </Button>
                        ),
                      },
                    ]),
              ]}
            />
          </div>
        ))}
        {exceptions.length > 0 && (
          <div>
            <div style={{ fontWeight: 600, margin: '10px 0 4px' }}>{t('lint.lib.exceptions')}</div>
            <Table<LintException>
              size="small"
              pagination={false}
              rowKey={(e) => `${e.clause}\n${e.object}`}
              dataSource={exceptions}
              columns={[
                {
                  title: t('lint.lib.clause'),
                  render: (_: unknown, e) => draft.clauses.find((c) => c.id === e.clause)?.text ?? e.clause,
                },
                {
                  title: t('lint.report.object_col'),
                  dataIndex: 'object',
                  width: 160,
                  render: (v: string) => <code>{v}</code>,
                },
                { title: t('lint.report.waive_reason'), dataIndex: 'reason' },
                {
                  title: '',
                  width: 70,
                  render: (_: unknown, e) => (
                    <Button type="link" size="small" onClick={() => removeException(e)}>
                      {t('lint.lib.delete')}
                    </Button>
                  ),
                },
              ]}
            />
          </div>
        )}
      </div>
    </div>
  )
}

function newManualClause(set: LintRuleset): LintClause {
  let n = set.clauses.length + 1
  let id = `custom.${n}`
  const ids = new Set(set.clauses.map((c) => c.id))
  while (ids.has(id)) id = `custom.${++n}`
  return { id, section: 'design', text: '', enabled: true, severity: 'info', manual_note: '', rules: [] }
}

function Params({
  clause,
  rules,
  onChange,
}: {
  clause: LintClause
  rules: LintRuleInfo[]
  onChange: (rules: NonNullable<LintClause['rules']>) => void
}) {
  const { t } = useTranslation()
  const list = clause.rules ?? []
  const setParam = (ri: number, key: string, value: unknown) => {
    const next = list.map((r, i) => (i === ri ? { ...r, params: { ...(r.params ?? {}), [key]: value } } : r))
    onChange(next)
  }
  return (
    <Space direction="vertical" size={6} style={{ width: '100%' }}>
      {list.map((rule, ri) => {
        const info = rules.find((x) => x.id === rule.id)
        return (
          <div key={ri} style={{ display: 'flex', gap: 10, alignItems: 'center', flexWrap: 'wrap' }}>
            <code style={{ minWidth: 180 }}>{rule.id}</code>
            {(info?.params ?? []).map((p) => {
              const val = rule.params?.[p.key] ?? p.default
              const label = <span style={{ color: '#888' }}>{p.key}</span>
              if (p.kind === 'number') {
                return (
                  <Space key={p.key} size={4}>
                    {label}
                    <InputNumber size="small" value={val as number} onChange={(v) => setParam(ri, p.key, v)} />
                  </Space>
                )
              }
              if (p.kind === 'choice') {
                return (
                  <Space key={p.key} size={4}>
                    {label}
                    <Select
                      size="small"
                      style={{ width: 110 }}
                      value={val as string}
                      onChange={(v) => setParam(ri, p.key, v)}
                      options={(p.choices ?? []).map((c) => ({ value: c, label: c }))}
                    />
                  </Space>
                )
              }
              if (p.kind === 'list') {
                return (
                  <Space key={p.key} size={4}>
                    {label}
                    <Select
                      size="small"
                      mode="tags"
                      style={{ minWidth: 160 }}
                      value={val as string[]}
                      onChange={(v) => setParam(ri, p.key, v)}
                    />
                  </Space>
                )
              }
              return (
                <Space key={p.key} size={4}>
                  {label}
                  <Input size="small" style={{ width: 140 }} value={val as string} onChange={(e) => setParam(ri, p.key, e.target.value)} />
                </Space>
              )
            })}
            {!info && <span style={{ color: '#c0392b' }}>{t('lint.lib.unknown_rule')}</span>}
          </div>
        )
      })}
    </Space>
  )
}
