// 导出文档对话框（M4c 设计文档 3.2 节）：文档种类、格式、范围、文档信息、模板；开始前检查，
// 设计文档先在屏幕外画 ER 图并上传，再开始后台任务，由右下角的通知跟踪。

import { useQuery } from '@tanstack/react-query'
import { App, Button, Checkbox, Form, Input, Modal, Progress, Radio, Segmented, Select, Space } from 'antd'
import { useEffect, useMemo, useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'

import { useApi } from '../../app/context'
import { DEFAULT_DIAGRAM, type DocFormat, type DocumentProblem, errorText, schemaLabel } from '../../shared/api'
import { useSettings } from '../../shared/settings'
import { desktop } from '../../shared/shell'
import { withVault } from '../connections/vault'
import { LARGE } from '../er/limits'
import { useDocExport } from './dialog'
import {
  type DocOptions,
  type DocTarget,
  buildRequest,
  extensionOf,
  fileName,
  fixFormat,
  formatsFor,
  loadOptions,
  optionsKey,
} from './options'
import type { DiagramJob } from './render'
import { useDocTracker } from './track'

const SEP = '\u0000'

function Dialog({ target }: { target: DocTarget }) {
  const api = useApi()
  const { t } = useTranslation()
  const { message } = App.useApp()
  const close = useDocExport((s) => s.close)
  const track = useDocTracker()
  const key = optionsKey(target)
  const [o, setO] = useState<DocOptions>(() => {
    const loaded = loadOptions(useSettings.getState().values[key], target)
    // 从 ER 图打开：设计文档
    return target.kind === 'model' && target.diagram ? { ...loaded, kind: 'design', format: fixFormat('design', loaded.format) } : loaded
  })
  const set = (p: Partial<DocOptions>) => setO((cur) => ({ ...cur, ...p }))
  const isModel = target.kind === 'model'
  const [schemas, setSchemas] = useState<string[]>(
    target.kind === 'model' ? (target.schema ? [target.schema] : target.schemas) : [target.schema],
  )
  const single = schemas.length === 1 ? schemas[0] : null
  const [tables, setTables] = useState<string[]>([])
  const [diagrams, setDiagrams] = useState<string[]>(
    target.kind === 'model' && target.diagram && target.diagram !== DEFAULT_DIAGRAM && target.schema
      ? [`${target.schema}${SEP}${target.diagram}`]
      : [],
  )
  const [problems, setProblems] = useState<DocumentProblem[]>([])
  const [busy, setBusy] = useState(false)
  const [drawing, setDrawing] = useState<{ done: number; total: number } | null>(null)
  const stop = useRef(false)

  useEffect(() => setTables([]), [single])

  const tablesQ = useQuery({
    queryKey: ['doc-tables', target.kind === 'model' ? target.model : target.profile, single],
    queryFn: async () =>
      target.kind === 'model'
        ? (await api.modelTables(target.model)).filter((e) => e.schema === single && !e.error).map((e) => e.name)
        : (await withVault(api, () => api.objects(target.profile, target.schema, 'tables'))).map((x) => x.name),
    enabled: !!single,
  })
  const diagramsQ = useQuery({
    queryKey: ['model-diagrams', target.kind === 'model' ? target.model : null, 'doc', schemas.join(SEP)],
    queryFn: async () => {
      if (target.kind !== 'model') return []
      const lists = await Promise.all(schemas.map((s) => api.diagrams(target.model, s)))
      return lists.flat().filter((d) => d.name !== DEFAULT_DIAGRAM)
    },
    enabled: isModel && o.kind === 'design',
  })
  const diagramOptions = useMemo(
    () => (diagramsQ.data ?? []).map((d) => ({ value: `${d.schema}${SEP}${d.name}`, label: `${d.schema} / ${d.name}` })),
    [diagramsQ.data],
  )

  const problem = (field: DocumentProblem['field']) => problems.find((p) => p.field === field)
  const status = (field: DocumentProblem['field']) => {
    const p = problem(field)
    return p ? { validateStatus: 'error' as const, help: errorText(p.error) } : {}
  }

  /** 设计文档要画的图：每个 schema 的默认图，加选中的主题图。 */
  const jobs = async (): Promise<DiagramJob[]> => {
    if (target.kind === 'database') {
      const list = await withVault(api, () => api.schemaTables(target.profile, target.schema))
      const picked = tables.length ? list.filter((x) => tables.includes(x.name)) : list
      const large = picked.length > LARGE
      return [
        {
          schema: target.schema,
          name: DEFAULT_DIAGRAM,
          title: schemaLabel(target.schema),
          description: null,
          tables: picked.map((table) => ({ table, logical_keys: [] })),
          display: large ? 'names' : 'columns',
          positions: {},
          namesOnly: large,
        },
      ]
    }
    const out: DiagramJob[] = []
    for (const schema of schemas) {
      const all = await api.diagram(target.model, schema, DEFAULT_DIAGRAM)
      const names = [DEFAULT_DIAGRAM, ...diagrams.filter((d) => d.startsWith(`${schema}${SEP}`)).map((d) => d.split(SEP)[1])]
      for (const name of names) {
        const v = name === DEFAULT_DIAGRAM ? all : await api.diagram(target.model, schema, name)
        const list = v.diagram.tables
        const pickedTables = name === DEFAULT_DIAGRAM && single && tables.length ? tables : null
        const desc = v.diagram.description
        out.push({
          schema,
          name,
          title: name === DEFAULT_DIAGRAM ? schema : name,
          description: typeof desc === 'string' ? desc : null,
          tables: all.tables.filter(
            (x) =>
              !x.error &&
              (list === 'all' || list.includes(x.table.name)) &&
              (!pickedTables || pickedTables.includes(x.table.name)),
          ),
          display: v.diagram.display,
          positions: v.diagram.positions ?? {},
          namesOnly: false,
        })
      }
    }
    return out
  }

  const submit = async () => {
    if (isModel && !schemas.length) return
    setBusy(true)
    setProblems([])
    stop.current = false
    try {
      useSettings.getState().set(key, JSON.stringify(o))
      const draft = buildRequest(target, o, schemas, tables, [], null)
      const found = await api.checkDocument(draft)
      if (found.length) {
        setProblems(found)
        return
      }
      let output: string | null = null
      if (desktop) {
        const ext = extensionOf(o.format)
        output = await desktop.saveDialog(fileName(o.title || draft.info.title, o.format), [
          { name: o.format.toUpperCase(), extensions: [ext] },
        ])
        if (!output) return
      }
      let images: Awaited<ReturnType<typeof import('./render').renderDiagrams>> = []
      if (o.kind === 'design') {
        const list = await jobs()
        setDrawing({ done: 0, total: list.length })
        const { renderDiagrams } = await import('./render')
        images = await renderDiagrams(
          api,
          list,
          (count) => t('er.columns', { count }),
          (done, total) => setDrawing({ done, total }),
          () => stop.current,
        )
        if (!images) return
      }
      const id = await api.startDocument(buildRequest(target, o, schemas, tables, images, output))
      close()
      void track(id, o.title.trim() || draft.info.title || t('document.title'), output)
    } catch (e) {
      message.error((e as Error).message)
    } finally {
      setBusy(false)
      setDrawing(null)
    }
  }

  const pickFile = async (field: 'template' | 'reference') => {
    if (!desktop) return
    const ext = field === 'reference' ? ['docx'] : [extensionOf(o.format)]
    const path = await desktop.openDialog([{ name: ext[0].toUpperCase(), extensions: ext }])
    if (path) set({ [field]: path })
  }

  const saveBuiltin = async () => {
    if (!desktop || (o.format !== 'html' && o.format !== 'markdown')) return
    const ext = extensionOf(o.format)
    const path = await desktop.saveDialog(`dblens-template.${ext}`, [{ name: ext.toUpperCase(), extensions: [ext] }])
    if (!path) return
    try {
      await api.exportDocTemplate(o.format, path)
      set({ template: path })
      message.success(t('document.templateSaved'))
    } catch (e) {
      message.error((e as Error).message)
    }
  }

  const textFormat = o.format === 'html' || o.format === 'markdown'
  const formatLabel: Record<DocFormat, string> = { docx: 'Word (docx)', html: 'HTML', markdown: 'Markdown', xlsx: 'Excel (xlsx)' }

  return (
    <Modal
      open
      width={620}
      title={t('document.title')}
      onCancel={() => {
        stop.current = true
        if (!busy) close()
      }}
      footer={
        drawing ? (
          <div className="row" style={{ gap: 12 }}>
            <span className="grow" style={{ textAlign: 'left' }}>
              {t('document.drawing', { done: drawing.done, total: drawing.total })}
              <Progress percent={Math.round((drawing.done / Math.max(1, drawing.total)) * 100)} size="small" />
            </span>
            <Button onClick={() => (stop.current = true)}>{t('document.cancel')}</Button>
          </div>
        ) : (
          <Space>
            <Button onClick={close}>{t('document.cancel')}</Button>
            <Button type="primary" loading={busy} disabled={isModel && !schemas.length} onClick={() => void submit()}>
              {t('document.export')}
            </Button>
          </Space>
        )
      }
    >
      <Form layout="horizontal" labelCol={{ flex: '96px' }} colon={false} disabled={busy}>
        <Form.Item label={t('document.kind')}>
          <Segmented
            value={o.kind}
            options={[
              { value: 'dictionary', label: t('document.dictionary') },
              { value: 'design', label: t('document.design') },
            ]}
            onChange={(v) => set({ kind: v as DocOptions['kind'], format: fixFormat(v as DocOptions['kind'], o.format) })}
          />
        </Form.Item>
        <Form.Item label={t('document.format')} {...status('format')}>
          <Radio.Group
            optionType="button"
            value={o.format}
            options={formatsFor(o.kind).map((f) => ({ value: f, label: formatLabel[f] }))}
            onChange={(e) => set({ format: e.target.value as DocFormat })}
          />
        </Form.Item>
        {target.kind === 'model' ? (
          <Form.Item label="Schema" required>
            <Select mode="multiple" value={schemas} onChange={setSchemas} options={target.schemas.map((s) => ({ value: s, label: s }))} />
          </Form.Item>
        ) : (
          <Form.Item label="Schema">
            <span>{schemaLabel(target.schema)}</span>
          </Form.Item>
        )}
        {single && (
          <Form.Item label={t('document.tables')} extra={t('document.tablesHint')}>
            <Select
              mode="multiple"
              allowClear
              showSearch
              value={tables}
              onChange={setTables}
              loading={tablesQ.isLoading}
              placeholder={t('document.allTables')}
              options={(tablesQ.data ?? []).map((n) => ({ value: n, label: n }))}
              maxTagCount="responsive"
            />
          </Form.Item>
        )}
        {o.kind === 'design' && isModel && (
          <Form.Item label={t('document.diagrams')} extra={t('document.diagramsHint')}>
            <Select
              mode="multiple"
              allowClear
              value={diagrams}
              onChange={setDiagrams}
              loading={diagramsQ.isLoading}
              placeholder={t('document.defaultDiagramsOnly')}
              options={diagramOptions}
            />
          </Form.Item>
        )}
        <Form.Item label={t('document.docTitle')}>
          <Input value={o.title} onChange={(e) => set({ title: e.target.value })} />
        </Form.Item>
        <Form.Item label={t('document.version')}>
          <Space.Compact style={{ width: '100%' }}>
            <Input value={o.version} onChange={(e) => set({ version: e.target.value })} />
            <Input value={o.author} placeholder={t('document.author')} onChange={(e) => set({ author: e.target.value })} />
          </Space.Compact>
        </Form.Item>
        <Form.Item label={t('document.description')}>
          <Input.TextArea autoSize={{ minRows: 2, maxRows: 5 }} value={o.description} onChange={(e) => set({ description: e.target.value })} />
        </Form.Item>
        {textFormat && (
          <Form.Item label={t('document.template')} {...status('template')}>
            <Space.Compact style={{ width: '100%' }}>
              <Input value={o.template} placeholder={t('document.builtinTemplate')} allowClear onChange={(e) => set({ template: e.target.value })} />
              {desktop && <Button onClick={() => void pickFile('template')}>{t('document.choose')}</Button>}
              {desktop && <Button onClick={() => void saveBuiltin()}>{t('document.saveBuiltin')}</Button>}
            </Space.Compact>
          </Form.Item>
        )}
        {o.format === 'docx' && (
          <Form.Item label={t('document.reference')} {...status('reference')} extra={t('document.referenceHint')}>
            <Space.Compact style={{ width: '100%' }}>
              <Input value={o.reference} placeholder={t('document.builtinReference')} allowClear onChange={(e) => set({ reference: e.target.value })} />
              {desktop && <Button onClick={() => void pickFile('reference')}>{t('document.choose')}</Button>}
            </Space.Compact>
          </Form.Item>
        )}
        {o.kind === 'design' && target.kind === 'database' && (
          <Form.Item label=" ">
            <Checkbox checked={o.viewDefinitions} onChange={(e) => set({ viewDefinitions: e.target.checked })}>
              {t('document.viewDefinitions')}
            </Checkbox>
          </Form.Item>
        )}
        {problem('output') && (
          <div className="err">{errorText(problem('output')!.error)}</div>
        )}
      </Form>
    </Modal>
  )
}

/** 挂在外壳中：打开时显示。 */
export default function ExportDocDialog() {
  const target = useDocExport((s) => s.target)
  return target ? <Dialog target={target} /> : null
}
