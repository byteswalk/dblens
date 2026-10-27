import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

import i18n from '../../shared/i18n'
import { useSettings } from '../../shared/settings'
import { TABS_KEY, parseTabs, persistTabs, useTabs } from './tabs'

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
