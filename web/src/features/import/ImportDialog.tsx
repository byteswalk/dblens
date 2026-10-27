// 导入向导（M2b 设计文档 3.2 节）：选文件和读取选项、目标与列映射（已有表或新建表）、写入选项。
// 提交后对话框关闭，右下角的通知跟踪任务（track.tsx）。

import { useQuery } from '@tanstack/react-query'
import {
  Alert,
  App,
  Button,
  Checkbox,
  Input,
  InputNumber,
  Modal,
  Radio,
  Select,
  Steps,
  Table,
  Tooltip,
} from 'antd'
import { IconAlertTriangle } from '@tabler/icons-react'
import { useEffect, useMemo, useState } from 'react'
import { useTranslation } from 'react-i18next'

import { useApi } from '../../app/context'
import type { FileOptions, ImportRequest, WriteMode } from '../../shared/api'
import { cellText } from '../../shared/exec'
import { withVault } from '../connections/vault'
import FileField, { type PickedFile } from './FileField'
import { useImportDialogs } from './dialog'
import {
  type ColumnDraft,
  IMPORT_EXTENSIONS,
  TYPE_CHOICES,
  autoMap,
  cellError,
  draftOf,
  draftProblem,
  fileFormat,
  modeAllowed,
  newTable,
  tableNameFromFile,
  upsertBlocked,
} from './logic'
import { useTaskTracker } from './track'

type Draft = ColumnDraft & { include: boolean }

const DEFAULT_OPTIONS: FileOptions = { format: 'csv', encoding: 'auto', delimiter: null, header: true, sheet: null }

export default function ImportDialog() {
  const api = useApi()
  const { t } = useTranslation()
  const { modal } = App.useApp()
  const target = useImportDialogs((s) => s.importTarget)
  const close = useImportDialogs((s) => s.closeImport)
  const track = useTaskTracker()
  const profileId = target?.profileId ?? ''

  const [step, setStep] = useState(0)
  const [file, setFile] = useState<PickedFile | null>(null)
  const [opts, setOpts] = useState<FileOptions>(DEFAULT_OPTIONS)
  const [schema, setSchema] = useState<string | null>(null)
  const [isNew, setIsNew] = useState(false)
  const [table, setTable] = useState<string | null>(null)
  const [newName, setNewName] = useState('')
  const [mapping, setMapping] = useState<(string | null)[]>([])
  const [drafts, setDrafts] = useState<Draft[]>([])
  const [mode, setMode] = useState<WriteMode>('append')
  const [transaction, setTransaction] = useState<'whole' | 'batch'>('batch')
  const [batchSize, setBatchSize] = useState(1000)
  const [onError, setOnError] = useState<'stop' | 'skip'>('stop')
  const [emptyAsString, setEmptyAsString] = useState(false)
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState<string | null>(null)

  // 每次打开从头开始
  useEffect(() => {
    if (!target) return
    setStep(0)
    setFile(null)
    setOpts(DEFAULT_OPTIONS)
    setSchema(target.schema)
    setIsNew(target.create)
    setTable(target.table)
    setNewName('')
    setMode('append')
    setError(null)
  }, [target])

  const { data: profiles = [] } = useQuery({ queryKey: ['profiles'], queryFn: () => api.profiles() })
  const prod = profiles.find((p) => p.id === profileId)?.environment === 'prod'
  const preview = useQuery({
    queryKey: ['import-preview', file?.source, opts],
    queryFn: () => withVault(api, () => api.importPreview(file!.source, opts)),
    enabled: !!target && !!file,
    retry: false,
  })
  const pv = preview.data
  const schemas = useQuery({
    queryKey: ['schemas', profileId],
    queryFn: () => withVault(api, () => api.schemas(profileId)),
    enabled: !!target,
  })
  const tables = useQuery({
    queryKey: ['import-tables', profileId, schema],
    queryFn: () => api.objects(profileId, schema!, 'tables'),
    enabled: !!target && !!schema && !isNew,
  })
  const info = useQuery({
    queryKey: ['import-table', profileId, schema, table],
    queryFn: () => api.table(profileId, schema!, table!),
    enabled: !!target && !!schema && !!table && !isNew,
  }).data

  // 预览到达：新建表的列取推断的类型，表名取文件名
  useEffect(() => {
    if (!pv) return
    setDrafts(pv.columns.map((c, k) => ({ ...draftOf(c, pv.types[k]), include: true })))
    if (file) setNewName((n) => n || tableNameFromFile(file.name))
  }, [pv, file])
  // 已有表：按列名自动映射
  useEffect(() => {
    if (pv && info) setMapping(autoMap(pv.columns, info.columns.map((c) => c.name)))
  }, [pv, info])

  const mapped = isNew ? drafts.filter((d) => d.include).map((d) => d.name.trim()) : mapping.filter((m): m is string => !!m)
  const blocked = isNew ? null : upsertBlocked(info ?? null, mapped)
  useEffect(() => {
    if (!modeAllowed(mode, isNew, blocked)) setMode('append')
  }, [mode, isNew, blocked])
  const problem = isNew ? draftProblem(drafts.filter((d) => d.include)) : null
  const ddl = useQuery({
    queryKey: ['import-ddl', profileId, schema, newName, drafts],
    queryFn: () =>
      api.importDdl(profileId, schema!, newName.trim(), newTable(drafts.filter((d) => d.include))),
    enabled: !!target && isNew && step === 1 && !!schema && !!newName.trim() && !problem && mapped.length > 0,
    retry: false,
  })

  // 已有表：每个映射的列在预览中解析失败的单元格
  const checks = useMemo(() => {
    if (!pv || !info || isNew) return []
    return mapping.map((m, k) => {
      const col = m ? info.columns.find((c) => c.name === m) : undefined
      if (!col) return { count: 0, first: null as string | null }
      const errors = pv.rows.map((r) => cellError(r[k] ?? { t: 'null' }, col)).filter((e): e is string => !!e)
      return { count: errors.length, first: errors[0] ?? null }
    })
  }, [pv, info, mapping, isNew])

  const canNext =
    step === 0
      ? !!pv
      : isNew
        ? !!schema && !!newName.trim() && mapped.length > 0 && !problem
        : !!schema && !!table && mapped.length > 0

  const submit = async () => {
    if (!target || !file || !schema) return
    const included = drafts.map((d, k) => ({ d, k })).filter((x) => x.d.include)
    const targetTable = isNew ? newName.trim() : table!
    const req: ImportRequest = {
      profile_id: profileId,
      source: file.source,
      file: opts,
      schema,
      table: targetTable,
      create: isNew ? newTable(included.map((x) => x.d)) : null,
      mapping: isNew
        ? included.map((x) => ({ source: x.k, target: x.d.name.trim() }))
        : mapping.flatMap((m, k) => (m ? [{ source: k, target: m }] : [])),
      mode,
      transaction: mode === 'replace' ? 'whole' : transaction,
      batch_size: batchSize,
      on_error: onError,
      empty_as_string: emptyAsString,
    }
    const start = async () => {
      setBusy(true)
      setError(null)
      try {
        const id = await withVault(api, () => api.startImport(req))
        close()
        void track(id, {
          title: t('import.task.importTitle', { file: file.name, table: targetTable }),
          profileId,
          schema,
          table: targetTable,
          refreshTree: isNew,
        })
      } catch (e) {
        setError((e as Error).message)
      } finally {
        setBusy(false)
      }
    }
    if (prod) {
      modal.confirm({
        title: t('import.options.prodTitle'),
        content: (
          <div>
            <div>{t('import.options.prodContent', { table: `${schema}.${targetTable}`, mode: t(`import.options.modes.${mode}`) })}</div>
            {mode === 'replace' && <div className="err">{t('import.options.replaceWarn')}</div>}
          </div>
        ),
        okType: 'danger',
        okText: t('import.options.start'),
        onOk: start,
      })
    } else await start()
  }

  const fileStep = (
    <div style={{ display: 'flex', flexDirection: 'column', gap: 12 }}>
      <FileField
        value={file}
        onChange={(f) => {
          const format = fileFormat(f.name)
          setFile(f)
          setNewName('')
          setOpts({ ...DEFAULT_OPTIONS, format: format ?? 'csv' })
        }}
        filters={[{ name: t('import.file.filter'), extensions: IMPORT_EXTENSIONS }]}
      />
      {file && fileFormat(file.name) === null && <Alert type="warning" showIcon message={t('import.file.unknownFormat')} />}
      {file && opts.format === 'csv' && (
        <div className="row" style={{ gap: 16, flexWrap: 'wrap' }}>
          <span>
            {t('import.file.encoding')}{' '}
            <Select
              size="small"
              value={opts.encoding ?? 'auto'}
              style={{ width: 120 }}
              onChange={(encoding) => setOpts((o) => ({ ...o, encoding }))}
              options={[
                { value: 'auto', label: pv?.encoding ? t('import.file.autoWith', { value: pv.encoding }) : t('import.file.auto') },
                { value: 'utf8', label: 'UTF-8' },
                { value: 'gbk', label: 'GBK' },
              ]}
            />
          </span>
          <span>
            {t('import.file.delimiter')}{' '}
            <Select
              size="small"
              value={opts.delimiter ?? 'auto'}
              style={{ width: 140 }}
              onChange={(d) => setOpts((o) => ({ ...o, delimiter: d === 'auto' ? null : d }))}
              options={[
                {
                  value: 'auto',
                  label: pv?.delimiter
                    ? t('import.file.autoWith', { value: delimiterName(pv.delimiter, t) })
                    : t('import.file.auto'),
                },
                { value: ',', label: t('import.file.delimiters.comma') },
                { value: ';', label: t('import.file.delimiters.semicolon') },
                { value: '\t', label: t('import.file.delimiters.tab') },
                { value: '|', label: t('import.file.delimiters.pipe') },
              ]}
            />
          </span>
          <Checkbox checked={opts.header !== false} onChange={(e) => setOpts((o) => ({ ...o, header: e.target.checked }))}>
            {t('import.file.header')}
          </Checkbox>
        </div>
      )}
      {file && opts.format === 'excel' && (
        <div className="row" style={{ gap: 16, flexWrap: 'wrap' }}>
          <span>
            {t('import.file.sheet')}{' '}
            <Select
              size="small"
              value={opts.sheet ?? pv?.sheets[0]}
              style={{ width: 180 }}
              onChange={(sheet) => setOpts((o) => ({ ...o, sheet }))}
              options={(pv?.sheets ?? []).map((s) => ({ value: s, label: s }))}
            />
          </span>
          <Checkbox checked={opts.header !== false} onChange={(e) => setOpts((o) => ({ ...o, header: e.target.checked }))}>
            {t('import.file.header')}
          </Checkbox>
          <span className="muted">{t('import.file.excelNote')}</span>
        </div>
      )}
      {preview.error && <Alert type="error" showIcon message={(preview.error as Error).message} />}
      {pv && (
        <>
          {(pv.warnings.ragged_rows > 0 || pv.warnings.bad_encoding > 0 || pv.warnings.error_cells > 0) && (
            <Alert
              type="warning"
              showIcon
              message={[
                pv.warnings.ragged_rows ? t('import.file.ragged', { count: pv.warnings.ragged_rows }) : '',
                pv.warnings.bad_encoding ? t('import.file.badEncoding', { count: pv.warnings.bad_encoding }) : '',
                pv.warnings.error_cells ? t('import.file.errorCells', { count: pv.warnings.error_cells }) : '',
              ]
                .filter(Boolean)
                .join(' ')}
            />
          )}
          <div className="muted">{t('import.file.previewNote', { count: pv.rows.length })}</div>
          <Table
            size="small"
            bordered
            pagination={false}
            scroll={{ x: 'max-content', y: 260 }}
            rowKey={(_, i) => String(i)}
            columns={pv.columns.map((c, k) => ({
              key: k,
              title: c,
              ellipsis: true,
              width: 140,
              render: (_: unknown, r: (typeof pv.rows)[number]) => (r[k]?.t === 'null' ? <span className="muted">NULL</span> : cellText(r[k])),
            }))}
            dataSource={pv.rows}
          />
        </>
      )}
    </div>
  )

  const targetStep = pv && (
    <div style={{ display: 'flex', flexDirection: 'column', gap: 12 }}>
      <div className="row" style={{ gap: 16, flexWrap: 'wrap' }}>
        <span>
          {t('import.target.schema')}{' '}
          <Select
            size="small"
            showSearch
            value={schema ?? undefined}
            style={{ width: 200 }}
            onChange={(s) => {
              setSchema(s)
              setTable(null)
            }}
            options={(schemas.data ?? []).map((s) => ({ value: s.name, label: s.name }))}
          />
        </span>
        <Radio.Group
          value={isNew ? 'new' : 'existing'}
          onChange={(e) => setIsNew(e.target.value === 'new')}
          options={[
            { value: 'existing', label: t('import.target.existing') },
            { value: 'new', label: t('import.target.new') },
          ]}
        />
        {isNew ? (
          <Input
            size="small"
            style={{ width: 220 }}
            value={newName}
            placeholder={t('import.target.newName')}
            onChange={(e) => setNewName(e.target.value)}
          />
        ) : (
          <Select
            size="small"
            showSearch
            value={table ?? undefined}
            placeholder={t('import.target.chooseTable')}
            style={{ width: 240 }}
            onChange={setTable}
            options={(tables.data ?? []).map((o) => ({ value: o.name, label: o.name }))}
          />
        )}
      </div>
      {!isNew && info && (
        <Table
          size="small"
          bordered
          pagination={false}
          scroll={{ y: 300 }}
          rowKey={(r) => String(r.k)}
          dataSource={pv.columns.map((c, k) => ({ k, c }))}
          columns={[
            { title: t('import.target.fileColumn'), dataIndex: 'c', width: 180, ellipsis: true },
            {
              title: t('import.target.sample'),
              width: 160,
              ellipsis: true,
              render: (_: unknown, r: { k: number }) => (pv.rows[0]?.[r.k] ? cellText(pv.rows[0][r.k]) : ''),
            },
            {
              title: t('import.target.tableColumn'),
              render: (_: unknown, r: { k: number }) => (
                <Select
                  size="small"
                  style={{ width: '100%' }}
                  value={mapping[r.k] ?? ''}
                  onChange={(v) => setMapping((m) => m.map((x, i) => (i === r.k ? v || null : x)))}
                  options={[
                    { value: '', label: t('import.target.skip') },
                    ...info.columns.map((col) => ({
                      value: col.name,
                      label: `${col.name}  ${col.data_type.native}`,
                      disabled: mapping.includes(col.name) && mapping[r.k] !== col.name,
                    })),
                  ]}
                />
              ),
            },
            {
              title: '',
              width: 48,
              render: (_: unknown, r: { k: number }) =>
                checks[r.k]?.count ? (
                  <Tooltip title={t('import.target.badCells', { count: checks[r.k].count, first: checks[r.k].first })}>
                    <IconAlertTriangle size={16} color="var(--danger)" />
                  </Tooltip>
                ) : null,
            },
          ]}
        />
      )}
      {isNew && (
        <>
          <Table
            size="small"
            bordered
            pagination={false}
            scroll={{ y: 260 }}
            rowKey={(_, i) => String(i)}
            dataSource={drafts}
            columns={[
              {
                title: '',
                width: 40,
                render: (_: unknown, d: Draft, i: number) => (
                  <Checkbox checked={d.include} onChange={(e) => updateDraft(i, { include: e.target.checked })} />
                ),
              },
              {
                title: t('import.target.columnName'),
                render: (_: unknown, d: Draft, i: number) => (
                  <Input size="small" value={d.name} onChange={(e) => updateDraft(i, { name: e.target.value })} />
                ),
              },
              {
                title: t('import.target.type'),
                width: 300,
                render: (_: unknown, d: Draft, i: number) => (
                  <span className="row" style={{ gap: 4 }}>
                    <Select
                      size="small"
                      style={{ width: 130 }}
                      value={d.type}
                      onChange={(type) => updateDraft(i, { type })}
                      options={TYPE_CHOICES.map((c) => ({ value: c, label: t(`import.types.${c}`) }))}
                    />
                    {d.type === 'var_char' && (
                      <InputNumber size="small" min={1} max={4000} value={d.len} onChange={(v) => updateDraft(i, { len: v ?? 50 })} />
                    )}
                    {d.type === 'decimal' && (
                      <>
                        <InputNumber size="small" min={1} max={38} value={d.precision} onChange={(v) => updateDraft(i, { precision: v ?? 10 })} />
                        <InputNumber size="small" min={0} max={38} value={d.scale} onChange={(v) => updateDraft(i, { scale: v ?? 0 })} />
                      </>
                    )}
                  </span>
                ),
              },
              {
                title: t('import.target.nullable'),
                width: 70,
                render: (_: unknown, d: Draft, i: number) => (
                  <Checkbox checked={d.nullable && !d.key} disabled={d.key} onChange={(e) => updateDraft(i, { nullable: e.target.checked })} />
                ),
              },
              {
                title: t('import.target.key'),
                width: 70,
                render: (_: unknown, d: Draft, i: number) => (
                  <Checkbox checked={d.key} onChange={(e) => updateDraft(i, { key: e.target.checked })} />
                ),
              },
            ]}
          />
          {problem && <Alert type="error" showIcon message={t(`import.target.problem.${problem}`)} />}
          {ddl.data && <pre className="mono ddl-preview">{ddl.data}</pre>}
          {ddl.error && <Alert type="error" showIcon message={(ddl.error as Error).message} />}
        </>
      )}
    </div>
  )

  function updateDraft(i: number, patch: Partial<Draft>) {
    setDrafts((ds) => ds.map((d, k) => (k === i ? { ...d, ...patch } : d)))
  }

  const reason = blocked ? t(`import.options.upsertBlocked.${blocked}`) : ''
  const optionsStep = (
    <div style={{ display: 'flex', flexDirection: 'column', gap: 14 }}>
      <div>
        <div className="form-label">{t('import.options.mode')}</div>
        <Radio.Group value={mode} onChange={(e) => setMode(e.target.value as WriteMode)}>
          {(['append', 'replace', 'upsert'] as const).map((m) => (
            <Tooltip key={m} title={m === 'upsert' && blocked ? reason : undefined}>
              <Radio value={m} disabled={!modeAllowed(m, isNew, blocked)}>
                {t(`import.options.modes.${m}`)}
              </Radio>
            </Tooltip>
          ))}
        </Radio.Group>
        {mode === 'replace' && <div className="err">{t('import.options.replaceWarn')}</div>}
        {mode === 'upsert' && info && profiles.find((p) => p.id === profileId)?.driver === 'mysql' && (
          <div className="muted">{t('import.options.mysqlUpsert')}</div>
        )}
      </div>
      <div>
        <div className="form-label">{t('import.options.transaction')}</div>
        <Radio.Group
          value={mode === 'replace' ? 'whole' : transaction}
          disabled={mode === 'replace'}
          onChange={(e) => setTransaction(e.target.value as 'whole' | 'batch')}
          options={[
            { value: 'batch', label: t('import.options.batch') },
            { value: 'whole', label: t('import.options.whole') },
          ]}
        />
        {transaction === 'batch' && mode !== 'replace' && (
          <div className="row" style={{ gap: 16, marginTop: 8, flexWrap: 'wrap' }}>
            <span>
              {t('import.options.batchSize')}{' '}
              <InputNumber size="small" min={100} max={100000} step={500} value={batchSize} onChange={(v) => setBatchSize(v ?? 1000)} />
            </span>
            <Radio.Group
              value={onError}
              onChange={(e) => setOnError(e.target.value as 'stop' | 'skip')}
              options={[
                { value: 'stop', label: t('import.options.stop') },
                { value: 'skip', label: t('import.options.skip') },
              ]}
            />
          </div>
        )}
        <div className="muted" style={{ marginTop: 6 }}>
          {mode === 'replace' || transaction === 'whole' ? t('import.options.wholeNote') : onError === 'skip' ? t('import.options.skipNote') : t('import.options.stopNote')}
        </div>
      </div>
      <div>
        <div className="form-label">{t('import.options.empty')}</div>
        <Radio.Group
          value={emptyAsString ? 'string' : 'null'}
          onChange={(e) => setEmptyAsString(e.target.value === 'string')}
          options={[
            { value: 'null', label: t('import.options.emptyNull') },
            { value: 'string', label: t('import.options.emptyString') },
          ]}
        />
      </div>
      {prod && <Alert type="error" showIcon message={t('import.options.prodNote')} />}
    </div>
  )

  const steps = [t('import.steps.file'), t('import.steps.target'), t('import.steps.options')]
  return (
    <Modal
      open={target !== null}
      title={t('import.title')}
      width={920}
      onCancel={close}
      destroyOnHidden
      footer={
        <span className="row" style={{ gap: 8, justifyContent: 'flex-end' }}>
          <Button onClick={close}>{t('import.cancel')}</Button>
          {step > 0 && <Button onClick={() => setStep(step - 1)}>{t('import.back')}</Button>}
          {step < 2 ? (
            <Button type="primary" disabled={!canNext} onClick={() => setStep(step + 1)}>
              {t('import.next')}
            </Button>
          ) : (
            <Button type="primary" danger={prod} loading={busy} onClick={() => void submit()}>
              {t('import.options.start')}
            </Button>
          )}
        </span>
      }
    >
      <Steps size="small" current={step} items={steps.map((title) => ({ title }))} style={{ marginBottom: 16 }} />
      {step === 0 && fileStep}
      {step === 1 && targetStep}
      {step === 2 && optionsStep}
      {error && <Alert type="error" showIcon message={error} style={{ marginTop: 12 }} />}
    </Modal>
  )
}

function delimiterName(d: string, t: (k: 'import.file.delimiters.comma' | 'import.file.delimiters.semicolon' | 'import.file.delimiters.tab' | 'import.file.delimiters.pipe') => string): string {
  if (d === ',') return t('import.file.delimiters.comma')
  if (d === ';') return t('import.file.delimiters.semicolon')
  if (d === '\t') return t('import.file.delimiters.tab')
  if (d === '|') return t('import.file.delimiters.pipe')
  return d
}
