import { describe, expect, it } from 'vitest'

import { joinCatalog } from '../../shared/api'
import i18n from '../../shared/i18n'
import {
  type NodeKey,
  categoriesFor,
  categoryLabel,
  decodeKey,
  encodeKey,
  isWithin,
  matches,
  qualifiedName,
  routineKindLabel,
} from './nodes'

describe('对象树节点', () => {
  it('各类数据库的分类与方言模板一致', () => {
    expect(categoriesFor('mysql').map((c) => c.kind)).toEqual(['tables', 'views', 'routines'])
    expect(categoriesFor('postgres').map((c) => c.label)).toEqual(['表', '视图', '物化视图', '函数和过程', '序列'])
    expect(categoriesFor('oracle').map((c) => c.kind)).toContain('synonyms')
    expect(categoriesFor('dameng').map((c) => c.kind)).toEqual(categoriesFor('oracle').map((c) => c.kind))
  })

  it('节点键编码后能原样解码，名字里有点、空格、中文也不受影响', () => {
    const keys: NodeKey[] = [
      { type: 'c', profileId: 'p1' },
      { type: 'g', profileId: 'p1' },
      { type: 's', profileId: 'p1', schema: 'my.schema' },
      { type: 'k', profileId: 'p1', schema: 'HR', kind: 'materialized_views' },
      { type: 'o', profileId: 'p1', schema: 'public', kind: 'tables', name: '订单 明细' },
      { type: 'o', profileId: 'p1', schema: 'HR', kind: 'routines', name: 'PKG', routineKind: 'package_body' },
      { type: 'l', profileId: 'p1', schema: 'public', table: 'orders', column: 'id' },
    ]
    for (const k of keys) expect(decodeKey(encodeKey(k))).toEqual(k)
    expect(() => decodeKey('x\u0001y')).toThrow('无法识别')
  })

  it('SQL Server 的库节点和库中的架构（M6a）', () => {
    const s = joinCatalog('销售', 'dbo')
    for (const k of [
      { type: 'd', profileId: 'p', catalog: '销售' },
      { type: 'g', profileId: 'p', catalog: '销售' },
      { type: 'g', profileId: 'p' },
      { type: 's', profileId: 'p', schema: s },
    ] as NodeKey[]) {
      expect(decodeKey(encodeKey(k))).toEqual(k)
    }
    expect(categoriesFor('sqlserver').map((c) => c.kind)).toEqual(['tables', 'views', 'routines', 'sequences', 'synonyms'])
    expect(qualifiedName({ type: 'o', profileId: 'p', schema: s, kind: 'tables', name: 't' })).toBe('销售.dbo.t')
    const db: NodeKey = { type: 'd', profileId: 'p', catalog: '销售' }
    expect(isWithin({ type: 's', profileId: 'p', schema: s }, db)).toBe(true)
    expect(isWithin({ type: 's', profileId: 'p', schema: joinCatalog('其他', 'dbo') }, db)).toBe(false)
    expect(isWithin({ type: 's', profileId: 'p', schema: s }, { type: 'g', profileId: 'p', catalog: '销售' })).toBe(true)
    expect(isWithin({ type: 's', profileId: 'p', schema: s }, { type: 'g', profileId: 'p' })).toBe(false)
    expect(isWithin(db, { type: 'c', profileId: 'p' })).toBe(true)
  })

  it('过滤按名称或注释，不区分大小写', () => {
    expect(matches('', 'orders')).toBe(true)
    expect(matches('ORD', 'orders')).toBe(true)
    expect(matches('订单', 'orders', '订单主表')).toBe(true)
    expect(matches('user', 'orders', null)).toBe(false)
  })

  it('复制名称与程序类型的中文名', () => {
    expect(qualifiedName({ type: 'o', profileId: 'p', schema: 'public', kind: 'tables', name: 'orders' })).toBe(
      'public.orders',
    )
    expect(qualifiedName({ type: 'l', profileId: 'p', schema: 'public', table: 'orders', column: 'id' })).toBe(
      'orders.id',
    )
    expect(routineKindLabel('package_body')).toBe('包体')
  })

  it('判断节点是否在另一个节点之下，用于刷新时丢掉下面已加载的内容', () => {
    const c: NodeKey = { type: 'c', profileId: 'p' }
    const s: NodeKey = { type: 's', profileId: 'p', schema: 'public' }
    const k: NodeKey = { type: 'k', profileId: 'p', schema: 'public', kind: 'tables' }
    const o: NodeKey = { type: 'o', profileId: 'p', schema: 'public', kind: 'tables', name: 'orders' }
    const l: NodeKey = { type: 'l', profileId: 'p', schema: 'public', table: 'orders', column: 'id' }
    expect([s, k, o, l].every((x) => isWithin(x, c))).toBe(true)
    expect(isWithin(c, c)).toBe(false)
    expect(isWithin({ ...s, profileId: 'q' }, c)).toBe(false)
    expect([k, o, l].every((x) => isWithin(x, s))).toBe(true)
    expect(isWithin({ ...k, schema: 'other' }, s)).toBe(false)
    expect(isWithin(o, k) && isWithin(l, k) && isWithin(l, o)).toBe(true)
    expect(isWithin(o, { ...k, kind: 'views' })).toBe(false)
    expect(isWithin({ ...l, table: 'users' }, o)).toBe(false)
    expect(isWithin(s, { type: 'g', profileId: 'p' })).toBe(true)
  })

  it('分类和程序类型的名称按调用时的界面语言翻译', async () => {
    try {
      await i18n.changeLanguage('en')
      expect(categoriesFor('postgres').map((c) => c.label)).toEqual([
        'Tables',
        'Views',
        'Materialized Views',
        'Routines',
        'Sequences',
      ])
      expect(categoryLabel('synonyms')).toBe('Synonyms')
      expect(routineKindLabel('package_body')).toBe('package body')
      expect(() => decodeKey('x\u0001y')).toThrow('Unrecognized node key')
    } finally {
      await i18n.changeLanguage('zh-CN')
    }
    expect(routineKindLabel('procedure')).toBe('过程')
  })
})
