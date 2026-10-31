// 导出文档对话框的选项（M4c 设计文档 3.2 节）：种类与格式的联动、按来源记住的选项、组成请求、图片的像素倍数。

import { schemaLabel } from '../../shared/api'
import type { DocDiagramImage, DocFormat, DocKind, DocSource, DocumentRequest } from '../../shared/api'
import { settingKey } from '../../shared/settings'

/** 从哪里打开的对话框。 */
export type DocTarget =
  | {
      kind: 'model'
      model: string
      path: string
      name: string
      description: string | null
      /** 模型的全部 schema。 */
      schemas: string[]
      /** 默认选中的 schema、ER 图（从 schema 节点或 ER 图打开时）。 */
      schema?: string
      diagram?: string
    }
  | { kind: 'database'; profile: string; schema: string }

/** 记住的选项。 */
export interface DocOptions {
  kind: DocKind
  format: DocFormat
  title: string
  version: string
  author: string
  description: string
  /** html、markdown 的模板文件；空为内置。 */
  template: string
  /** docx 的样式来源；空为内置。 */
  reference: string
  viewDefinitions: boolean
}

export const DEFAULT_OPTIONS: DocOptions = {
  kind: 'dictionary',
  format: 'docx',
  title: '',
  version: '',
  author: '',
  description: '',
  template: '',
  reference: '',
  viewDefinitions: true,
}

/** 某种文档可选的格式：xlsx 只用于数据字典。 */
export function formatsFor(kind: DocKind): DocFormat[] {
  return kind === 'dictionary' ? ['docx', 'html', 'markdown', 'xlsx'] : ['docx', 'html', 'markdown']
}

/** 换文档种类后格式不可用时换成 docx。 */
export function fixFormat(kind: DocKind, format: DocFormat): DocFormat {
  return formatsFor(kind).includes(format) ? format : 'docx'
}

export function extensionOf(format: DocFormat): string {
  return format === 'markdown' ? 'md' : format
}

/** 记住选项用的设置项：模型按目录，库按连接和 schema。 */
export function optionsKey(target: DocTarget): string {
  return settingKey(target.kind === 'model' ? `ui.doc.m.${target.path}` : `ui.doc.d.${target.profile}.${target.schema}`)
}

/** 读记住的选项；没有或格式不对时用默认值，模型的标题、说明默认取模型的名称、说明。 */
export function loadOptions(raw: string | undefined, target: DocTarget): DocOptions {
  const base: DocOptions = {
    ...DEFAULT_OPTIONS,
    title: target.kind === 'model' ? target.name : schemaLabel(target.schema),
    description: target.kind === 'model' ? (target.description ?? '') : '',
  }
  if (!raw) return base
  try {
    const saved = JSON.parse(raw) as Partial<DocOptions>
    const o = { ...base }
    for (const k of Object.keys(base) as (keyof DocOptions)[]) {
      if (typeof saved[k] === typeof base[k]) (o as Record<string, unknown>)[k] = saved[k]
    }
    o.format = fixFormat(o.kind, o.format)
    return o
  } catch {
    return base
  }
}

const orNull = (s: string) => (s.trim() ? s.trim() : null)

/** 组成导出请求。`tables` 为空数组时为全部表。 */
export function buildRequest(
  target: DocTarget,
  o: DocOptions,
  schemas: string[],
  tables: string[],
  diagrams: DocDiagramImage[],
  output: string | null,
): DocumentRequest {
  const source: DocSource =
    target.kind === 'model'
      ? { kind: 'model', model: target.model, schemas }
      : { kind: 'database', profile: target.profile, schema: target.schema }
  const single = target.kind === 'database' || schemas.length === 1
  return {
    source,
    tables: single && tables.length ? tables : null,
    kind: o.kind,
    format: o.format,
    info: { title: o.title.trim(), version: orNull(o.version), author: orNull(o.author), description: orNull(o.description) },
    template: o.format === 'html' || o.format === 'markdown' ? orNull(o.template) : null,
    reference: o.format === 'docx' ? orNull(o.reference) : null,
    view_definitions: o.viewDefinitions,
    diagrams: o.kind === 'design' ? diagrams : [],
    output,
  }
}

/** 文件名中不能用的字符换成下划线。 */
export function fileName(title: string, format: DocFormat): string {
  const base = title.replace(/[\\/:*?"<>|]/g, '_').trim() || 'document'
  return `${base}.${extensionOf(format)}`
}

/** 图片任一边的像素上限。 */
export const MAX_PIXELS = 8000

/**
 * 画 ER 图图片的像素倍数：2 倍不超过上限时用 2；1 倍不超过时用 1；仍超过时返回 0（改为只显示表名重画）。
 */
export function pixelRatio(width: number, height: number): 0 | 1 | 2 {
  const side = Math.max(width, height)
  if (side * 2 <= MAX_PIXELS) return 2
  return side <= MAX_PIXELS ? 1 : 0
}
