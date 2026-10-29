import { describe, expect, it } from 'vitest'

import {
  DEFAULT_OPTIONS,
  type DocTarget,
  buildRequest,
  fileName,
  fixFormat,
  formatsFor,
  loadOptions,
  optionsKey,
  pixelRatio,
} from './options'

const model: DocTarget = {
  kind: 'model',
  model: 'm1',
  path: 'D:/models/Shop',
  name: '商城',
  description: '订单库',
  schemas: ['public', 'crm'],
}
const db: DocTarget = { kind: 'database', profile: 'p1', schema: 'sales' }

describe('导出文档的选项', () => {
  it('xlsx 只用于数据字典，换成设计文档时改用 docx', () => {
    expect(formatsFor('dictionary')).toContain('xlsx')
    expect(formatsFor('design')).not.toContain('xlsx')
    expect(fixFormat('design', 'xlsx')).toBe('docx')
    expect(fixFormat('design', 'markdown')).toBe('markdown')
  })

  it('按来源记住选项：模型按目录，库按连接和 schema；默认标题取模型名或 schema', () => {
    expect(optionsKey(model)).toBe('ui.doc.m.d--models-shop')
    expect(optionsKey(db)).toBe('ui.doc.d.p1.sales')
    expect(loadOptions(undefined, model)).toMatchObject({ title: '商城', description: '订单库', kind: 'dictionary' })
    expect(loadOptions(undefined, db).title).toBe('sales')
    const saved = JSON.stringify({ kind: 'design', format: 'xlsx', author: '张三', viewDefinitions: 'yes' })
    // 格式随种类修正；类型不对的字段用默认值
    expect(loadOptions(saved, db)).toMatchObject({ kind: 'design', format: 'docx', author: '张三', viewDefinitions: true })
    expect(loadOptions('{bad', db).kind).toBe('dictionary')
  })

  it('组成请求：只选一个 schema 时才带表；模板只给对应的格式；图片只给设计文档', () => {
    const o = { ...DEFAULT_OPTIONS, title: ' 商城 ', version: '', template: 'D:/t.md', reference: 'D:/r.docx' }
    const img = { upload: 'u1', schema: 'public', name: '_all', title: 'public', names_only: false }
    const r = buildRequest(model, { ...o, format: 'markdown' }, ['public'], ['orders'], [img], null)
    expect(r.source).toEqual({ kind: 'model', model: 'm1', schemas: ['public'] })
    expect(r.tables).toEqual(['orders'])
    expect(r.info).toEqual({ title: '商城', version: null, author: null, description: null })
    expect(r.template).toBe('D:/t.md')
    expect(r.reference).toBeNull()
    expect(r.diagrams).toEqual([])
    const r2 = buildRequest(model, { ...o, kind: 'design', format: 'docx' }, ['public', 'crm'], ['orders'], [img], 'D:/out.docx')
    expect(r2.tables).toBeNull()
    expect(r2.reference).toBe('D:/r.docx')
    expect(r2.template).toBeNull()
    expect(r2.diagrams).toEqual([img])
    expect(r2.output).toBe('D:/out.docx')
    expect(buildRequest(db, o, [], [], [], null).source).toEqual({ kind: 'database', profile: 'p1', schema: 'sales' })
  })

  it('文件名和图片的像素倍数', () => {
    expect(fileName('a/b:c', 'markdown')).toBe('a_b_c.md')
    expect(fileName(' ', 'xlsx')).toBe('document.xlsx')
    expect(pixelRatio(4000, 1000)).toBe(2)
    expect(pixelRatio(4001, 1000)).toBe(1)
    expect(pixelRatio(1000, 8001)).toBe(0)
  })
})
