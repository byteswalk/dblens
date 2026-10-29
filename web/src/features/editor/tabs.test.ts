import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

import i18n from '../../shared/i18n'
import { useSettings } from '../../shared/settings'
import { TABS_KEY, markRun, parseTabs, persistTabs, takeRun, useTabs } from './tabs'

describe('标签页', () => {
  beforeEach(() => useTabs.getState().restore(undefined))

  it('新建的标签按顺序编号并成为当前标签', () => {
    const { open } = useTabs.getState()
    const a = open('p1')
    const b = open('p2', 'select 1')
    const s = useTabs.getState()
    expect(s.tabs.map((t) => [t.title, t.profileId, t.sql])).toEqual([
      ['查询1', 'p1', ''],
      ['查询2', 'p2', 'select 1'],
    ])
    expect(s.active).toBe(b)
    expect(a).not.toBe(b)
  })

  it('新标签的默认标题按当前界面语言生成', async () => {
    await i18n.changeLanguage('en')
    try {
      useTabs.getState().open('p')
      expect(useTabs.getState().tabs[0].title).toBe('Query 1')
    } finally {
      await i18n.changeLanguage('zh-CN')
    }
  })

  it('关闭当前标签时切到右边的，没有就左边的', () => {
    const { open, close, activate } = useTabs.getState()
    const [a, b, c] = [open('p'), open('p'), open('p')]
    activate(b)
    close(b)
    expect(useTabs.getState().active).toBe(c)
    close(c)
    expect(useTabs.getState().active).toBe(a)
    close(a)
    expect(useTabs.getState().active).toBeNull()
  })

  it('关闭其他标签不改变当前标签', () => {
    const { open, close } = useTabs.getState()
    const a = open('p')
    const b = open('p')
    close(a)
    expect(useTabs.getState().active).toBe(b)
  })

  it('解析保存的标签：丢弃格式不对的项', () => {
    const raw = JSON.stringify({
      tabs: [{ id: 'x', title: '查询3', profileId: 'p', sql: 'select 1' }, { id: 1 }],
      active: 'gone',
      seq: 4,
    })
    expect(parseTabs(raw)).toEqual({
      tabs: [{ id: 'x', title: '查询3', profileId: 'p', sql: 'select 1' }],
      active: 'x',
      seq: 4,
    })
    expect(parseTabs('not json')).toEqual({ tabs: [], active: null, seq: 1 })
    const data = { schema: 's', object: 't', objectKind: 'view', filter: 'a > 1', order: { column: 'a', desc: true } }
    const withData = JSON.stringify({
      tabs: [
        { id: 'd', title: 't', profileId: 'p', sql: '', data },
        { id: 'bad', title: 't', profileId: 'p', sql: '', data: { schema: 's', object: 't', objectKind: 'index' } },
      ],
    })
    expect(parseTabs(withData).tabs).toEqual([{ id: 'd', title: 't', profileId: 'p', sql: '', data }])
    expect(parseTabs(undefined).tabs).toEqual([])
  })
})

describe('数据标签', () => {
  beforeEach(() => useTabs.getState().restore(undefined))

  it('标题为对象名，不占查询编号；同一对象只开一个', () => {
    const { open, openData, activate } = useTabs.getState()
    const q = open('p')
    const a = openData('p', { schema: 's', object: 'orders', objectKind: 'table' })
    activate(q)
    expect(openData('p', { schema: 's', object: 'orders', objectKind: 'table' })).toBe(a)
    expect(useTabs.getState().active).toBe(a)
    const b = openData('p2', { schema: 's', object: 'orders', objectKind: 'table' })
    expect(b).not.toBe(a)
    open('p')
    expect(useTabs.getState().tabs.map((x) => x.title)).toEqual(['查询1', 'orders', 'orders', '查询2'])
  })

  it('修改过滤和排序；查询标签不受影响', () => {
    const { open, openData, setData } = useTabs.getState()
    const q = open('p')
    const d = openData('p', { schema: 's', object: 'v', objectKind: 'view' })
    setData(d, { filter: 'x = 1', order: { column: 'x', desc: false } })
    setData(q, { filter: 'y' })
    const tabs = useTabs.getState().tabs
    expect(tabs[1].data).toEqual({
      schema: 's',
      object: 'v',
      objectKind: 'view',
      filter: 'x = 1',
      order: { column: 'x', desc: false },
    })
    expect(tabs[0].data).toBeUndefined()
  })
})

describe('打开后执行', () => {
  it('设计器标签：同一张表只开一个，新建表每次一个；保存新表后转为编辑这张表', () => {
    useTabs.getState().restore(undefined)
    const { openDesign, setDesign } = useTabs.getState()
    const a = openDesign('p', { schema: 'public', table: 'orders' })
    expect(openDesign('p', { schema: 'public', table: 'orders' })).toBe(a)
    const n1 = openDesign('p', { schema: 'public', table: null })
    const n2 = openDesign('p', { schema: 'public', table: null })
    expect(n1).not.toBe(n2)
    const tabs = useTabs.getState().tabs
    expect(tabs.map((x) => x.title)).toEqual(['orders', '新表', '新表'])
    setDesign(n1, { schema: 'public', table: 'items' })
    expect(useTabs.getState().tabs[1]).toMatchObject({ title: 'items', design: { schema: 'public', table: 'items' } })
    // 保存再恢复
    const raw = JSON.stringify({ tabs: useTabs.getState().tabs, active: a, seq: 1 })
    expect(parseTabs(raw).tabs).toHaveLength(3)
    expect(parseTabs(JSON.stringify({ tabs: [{ id: 'x', title: 't', profileId: 'p', sql: '', design: { schema: 1 } }] })).tabs).toEqual([])
  })

  it('模型中的表与库中同名的表各开一个；比对标签保存后恢复', () => {
    useTabs.getState().restore(undefined)
    const { openDesign, openCompare, setCompare } = useTabs.getState()
    const db = openDesign('p', { schema: 'public', table: 'orders' })
    const m = openDesign('', { schema: 'public', table: 'orders', model: 'D:/m' })
    expect(m).not.toBe(db)
    expect(openDesign('', { schema: 'public', table: 'orders', model: 'D:/m' })).toBe(m)
    const c = openCompare({ source: { kind: 'model', path: 'D:/m', schema: 'public' }, target: null })
    setCompare(c, { source: null, target: { kind: 'database', profile: 'p', schema: 'shop' } })
    const raw = JSON.stringify({ tabs: useTabs.getState().tabs, active: c, seq: 1 })
    const back = parseTabs(raw)
    expect(back.tabs).toHaveLength(3)
    expect(back.tabs[2]).toMatchObject({ title: '比对', profileId: '', compare: { source: null, target: { kind: 'database', schema: 'shop' } } })
    const bad = { id: 'x', title: 't', profileId: '', sql: '', compare: { source: { kind: 'model', schema: 's' }, target: null } }
    expect(parseTabs(JSON.stringify({ tabs: [bad] })).tabs).toEqual([])
  })

  it('ER 图标签：同一张图只开一个，改名后跟着改，保存后恢复', () => {
    useTabs.getState().restore(undefined)
    const { openEr, setEr } = useTabs.getState()
    const target = { kind: 'model' as const, path: 'D:/m', schema: 'public', diagram: '_all' }
    const a = openEr(target, 'public · 全部表')
    expect(openEr({ ...target }, 'x')).toBe(a)
    const t1 = openEr({ ...target, diagram: '订单' }, 'public · 订单')
    setEr(t1, { ...target, diagram: '订单2' }, 'public · 订单2')
    const db = openEr({ kind: 'database', profile: 'p', schema: 'shop' }, 'shop · ER')
    const tabs = useTabs.getState().tabs
    expect(tabs.map((x) => [x.title, x.profileId])).toEqual([['public · 全部表', ''], ['public · 订单2', ''], ['shop · ER', 'p']])
    const back = parseTabs(JSON.stringify({ tabs, active: db, seq: 1 }))
    expect(back.tabs[1].er).toEqual({ ...target, diagram: '订单2' })
    const bad = { id: 'x', title: 't', profileId: '', sql: '', er: { kind: 'model', path: 'D:/m', schema: 's' } }
    expect(parseTabs(JSON.stringify({ tabs: [bad] })).tabs).toEqual([])
  })

  it('标记只取走一次，不随标签保存', () => {
    const id = useTabs.getState().open('p', 'SELECT 1')
    markRun(id)
    const saved = JSON.stringify({ tabs: useTabs.getState().tabs })
    expect(saved).not.toContain('run')
    expect(takeRun(id)).toBe(true)
    expect(takeRun(id)).toBe(false)
  })
})

describe('保存标签', () => {
  beforeEach(() => vi.useFakeTimers())
  afterEach(() => vi.useRealTimers())

  it('变化后延迟保存一次', () => {
    const save = vi.fn(() => Promise.resolve())
    useSettings.getState().init({}, save)
    useTabs.getState().restore(undefined)
    const stop = persistTabs(100)
    const id = useTabs.getState().open('p')
    useTabs.getState().setSql(id, 's')
    useTabs.getState().setSql(id, 'se')
    vi.advanceTimersByTime(150)
    stop()
    expect(save).toHaveBeenCalledTimes(1)
    const [key, value] = save.mock.calls[0] as unknown as [string, string]
    expect(key).toBe(TABS_KEY)
    expect(parseTabs(value).tabs[0].sql).toBe('se')
  })
})

describe('迁移标签', () => {
  it('恢复时给旧方案补上模式和数据选项', () => {
    const old = {
      source: { profile: 'a', schema: 's' },
      target: { profile: 'b', schema: 't' },
      tables: null,
      sequences: null,
      settings: { options: { name_case: null, comments: true, checks: true }, type_rules: [], tables: {}, sequences: {} },
      existing: {},
    }
    const raw = JSON.stringify({ tabs: [{ id: 'm', title: '迁移', profileId: '', sql: '', migrate: old }], active: 'm', seq: 2 })
    const plan = parseTabs(raw).tabs[0].migrate
    expect(plan?.mode).toBe('all')
    expect(plan?.data.batch_size).toBe(1000)
  })
})
