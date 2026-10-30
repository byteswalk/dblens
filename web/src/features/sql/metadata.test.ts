import { QueryClient } from '@tanstack/react-query'
import { describe, expect, it, vi } from 'vitest'

import { type MetaApi, defaultSchema, findName, metaKey, metaReader } from './metadata'

function fakeApi(): MetaApi & { calls: string[] } {
  const calls: string[] = []
  const log = <T>(name: string, v: T) => {
    calls.push(name)
    return Promise.resolve(v)
  }
  return {
    calls,
    schemas: () => log('schemas', [{ name: 'HR', is_system: false }]),
    objects: (_id, s, kind) => log(`objects ${s} ${kind}`, kind === 'tables' ? [{ name: 'EMP', comment: null }] : [{ name: 'V1', comment: '视图' }]),
    table: (_id, s, t) =>
      log(`table ${s}.${t}`, {
        name: t,
        comment: null,
        primary_key: null,
        columns: [{ name: 'ID', ordinal: 1, data_type: { native: 'NUMBER', logical: null }, nullable: false, default: null, comment: null }],
      }),
    synonyms: (_id, s) => log(`synonyms ${s}`, [{ name: 'S1', target_schema: null, target_name: 'X', db_link: null }]),
    sequences: (_id, s) => log(`sequences ${s}`, [{ name: 'Q1' }]),
    routines: (_id, s) =>
      log(`routines ${s}`, [
        { name: 'F1', kind: 'function' as const },
        { name: 'P1', kind: 'procedure' as const },
      ]),
  }
}

describe('metaReader', () => {
  it('取对象（Oracle、达梦另取同义词和序列）并缓存', async () => {
    const qc = new QueryClient()
    const api = fakeApi()
    const m = metaReader(qc, api, 'p1', 'oracle')
    const objs = await m.objects('HR')
    expect(objs.map((o) => `${o.kind}:${o.name}`)).toEqual(['table:EMP', 'view:V1', 'synonym:S1', 'sequence:Q1'])
    await m.objects('HR')
    expect(api.calls.filter((c) => c.startsWith('objects'))).toHaveLength(2)
    expect(await m.columns('HR', 'EMP')).toEqual([{ name: 'ID', type: 'NUMBER', comment: null }])
    expect(await m.functions('HR')).toEqual(['F1'])
    expect(await m.schemas()).toEqual(['HR'])
  })

  it('SQL Server：在会话的当前库中取架构和对象（M6a）', async () => {
    const api = fakeApi()
    const calls: string[] = []
    api.schemas = (_id, _f, catalog) => {
      calls.push(`schemas ${catalog}`)
      return Promise.resolve([{ name: 'dbo', is_system: false }])
    }
    const m = metaReader(new QueryClient(), api, 'p1', 'sqlserver', 'sales')
    await m.objects('dbo')
    expect(api.calls).toEqual(['objects sales\u001fdbo tables', 'objects sales\u001fdbo views', 'synonyms sales\u001fdbo', 'sequences sales\u001fdbo'])
    expect(await m.schemas()).toEqual(['dbo'])
    expect(calls).toEqual(['schemas sales'])
    expect(defaultSchema('sqlserver', 'sa', 'sales')).toBe('dbo')
  })

  it('MySQL 不取同义词和序列', async () => {
    const api = fakeApi()
    await metaReader(new QueryClient(), api, 'p1', 'mysql').objects('db')
    expect(api.calls).toEqual(['objects db tables', 'objects db views'])
  })

  it('失效后重新取', async () => {
    const qc = new QueryClient()
    const api = fakeApi()
    const m = metaReader(qc, api, 'p1', 'mysql')
    await m.columns('db', 't')
    await qc.invalidateQueries({ queryKey: metaKey('p1', 'db') })
    await m.columns('db', 't')
    expect(api.calls).toEqual(['table db.t', 'table db.t'])
  })

  it('出错或太慢时先返回空', async () => {
    vi.useFakeTimers()
    try {
      const api = fakeApi()
      api.table = () => new Promise(() => {})
      api.schemas = () => Promise.reject(new Error('locked'))
      const m = metaReader(new QueryClient(), api, 'p1', 'mysql', null, 100)
      const cols = m.columns('db', 't')
      await vi.advanceTimersByTimeAsync(101)
      await expect(cols).resolves.toEqual([])
      await expect(m.schemas()).resolves.toEqual([])
    } finally {
      vi.useRealTimers()
    }
  })
})

describe('findName / defaultSchema', () => {
  it('先找完全相同的，再不分大小写', () => {
    expect(findName(['emp', 'EMP'], 'EMP')).toBe('EMP')
    expect(findName([{ name: 'EMP' }], 'emp')).toEqual({ name: 'EMP' })
    expect(findName(['A'], 'b')).toBeUndefined()
  })

  it('当前 schema 的推断', () => {
    expect(defaultSchema('mysql', 'root', 'shop')).toBe('shop')
    expect(defaultSchema('mysql', 'root', null)).toBeNull()
    expect(defaultSchema('postgres', 'admin', 'db')).toBe('public')
    expect(defaultSchema('oracle', 'dblens_test')).toBe('DBLENS_TEST')
  })
})
