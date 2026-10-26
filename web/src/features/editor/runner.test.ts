import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

import type { Api, SessionInfo } from '../../shared/api'
import type { ServerEvent } from '../../shared/exec'
import { useSettings } from '../../shared/settings'
import { ROW_CAP_KEY, Runner, emptyRuntime, resumeAfterConfirm, shiftEvent, useRuntime } from './runner'
import { useTabs } from './tabs'

/** 记录发出的消息，测试里手动推送服务端事件。 */
class FakeWs extends EventTarget {
  static last: FakeWs
  sent: Record<string, unknown>[] = []
  constructor() {
    super()
    FakeWs.last = this
    queueMicrotask(() => this.dispatchEvent(new Event('open')))
  }
  send(m: string) {
    this.sent.push(JSON.parse(m))
  }
  close() {}
  push(ev: ServerEvent) {
    this.dispatchEvent(new MessageEvent('message', { data: JSON.stringify(ev) }))
  }
}

const session: SessionInfo = {
  id: 7,
  profile_id: 'p',
  variant: 'postgres',
  autocommit: true,
  in_transaction: false,
  busy: false,
  broken: false,
  schema: 'public',
}

function fakeApi() {
  return {
    openSession: vi.fn(async () => session),
    session: vi.fn(async () => session),
    wsUrl: () => 'ws://x',
    secrets: vi.fn(),
    closeSession: vi.fn(async () => null),
  } as unknown as Api & { openSession: ReturnType<typeof vi.fn> }
}

const flush = () => new Promise((r) => setTimeout(r, 0))
const rt = (id: string) => useRuntime.getState().tabs[id] ?? emptyRuntime

describe('执行器', () => {
  const real = globalThis.WebSocket
  let tabId: string
  beforeEach(() => {
    globalThis.WebSocket = FakeWs as unknown as typeof WebSocket
    useTabs.getState().restore(undefined)
    tabId = useTabs.getState().open('p')
    useSettings.getState().init({}, async () => null)
  })
  afterEach(() => {
    globalThis.WebSocket = real
  })

  it('第一次执行时打开会话，偏移换成编辑器全文中的位置', async () => {
    const api = fakeApi()
    const r = new Runner(api, tabId)
    r.setContinueOnError(true)
    await r.run('select 1', 20, { cursor: 3 })
    expect(api.openSession).toHaveBeenCalledWith('p')
    expect(FakeWs.last.sent[0]).toMatchObject({ sql: 'select 1', cursor: 3, continue_on_error: true })
    expect(rt(tabId).running).toBe(true)
    FakeWs.last.push({ type: 'statement_start', index: 0, total: 1, start: 0, end: 8, info: { keyword: 'SELECT' } })
    FakeWs.last.push({ type: 'failed', index: 0, error: { kind: 'sql' }, offset: 2 })
    FakeWs.last.push({ type: 'done' })
    const st = rt(tabId).exec.statements[0]
    expect([st.start, st.end, st.errorOffset]).toEqual([20, 28, 22])
    expect(rt(tabId).running).toBe(false)
    // 第二次执行复用会话
    await r.run('select 2', 0)
    expect(api.openSession).toHaveBeenCalledTimes(1)
  })

  it('确认后从需要确认的那条语句接着执行', async () => {
    const r = new Runner(fakeApi(), tabId)
    const sql = 'insert into t values (1);\ndelete from t'
    await r.run(sql, 100)
    const at = sql.indexOf('delete')
    FakeWs.last.push({ type: 'confirm_required', index: 1, start: at, end: sql.length, reason: '没有 WHERE' })
    FakeWs.last.push({ type: 'done' })
    expect(rt(tabId).exec.confirm?.start).toBe(100 + at)
    await r.confirm()
    expect(FakeWs.last.sent[1]).toMatchObject({ sql: 'delete from t', confirmed: true })
    expect(rt(tabId).submission?.base).toBe(100 + at)
    expect(rt(tabId).exec.confirm).toBeNull()
  })

  it('取消确认后关掉确认框，下次执行时重置', async () => {
    const r = new Runner(fakeApi(), tabId)
    await r.run('delete from t', 0)
    FakeWs.last.push({ type: 'confirm_required', index: 0, start: 0, end: 13, reason: '没有 WHERE' })
    FakeWs.last.push({ type: 'done' })
    r.dismissConfirm()
    expect([rt(tabId).declined, rt(tabId).exec.confirm?.reason]).toEqual([true, '没有 WHERE'])
    await r.run('select 1', 0)
    expect(rt(tabId).declined).toBe(false)
  })

  it('达到行数上限时取消，不再续取', async () => {
    useSettings.getState().init({ [ROW_CAP_KEY]: '1000' }, async () => null)
    const r = new Runner(fakeApi(), tabId)
    await r.run('select * from big', 0)
    const ws = FakeWs.last
    const batch = Array.from({ length: 500 }, (_, i) => [{ t: 'int', v: i }])
    ws.push({ type: 'statement_start', index: 0, total: 1, start: 0, end: 17, info: { keyword: 'SELECT' } })
    ws.push({ type: 'result_start', columns: [{ name: 'n', type_name: 'int', nullable: false }] })
    ws.push({ type: 'rows', rows: batch })
    r.fetchMore()
    ws.push({ type: 'rows', rows: batch })
    await flush()
    expect(ws.sent.map((m) => m.type)).toEqual(['exec', 'fetch_more', 'cancel'])
    expect(rt(tabId).capped).toBe(0)
    r.fetchMore()
    expect(ws.sent).toHaveLength(3)
  })

  it('不是最后一条语句时自动取完结果，最后一条等滚动', async () => {
    const r = new Runner(fakeApi(), tabId)
    await r.run('select 1; select 2', 0)
    const ws = FakeWs.last
    const batch = Array.from({ length: 500 }, (_, i) => [{ t: 'int', v: i }])
    const col = { name: 'n', type_name: 'int', nullable: false }
    ws.push({ type: 'statement_start', index: 0, total: 2, start: 0, end: 8, info: { keyword: 'SELECT' } })
    ws.push({ type: 'result_start', columns: [col] })
    ws.push({ type: 'rows', rows: batch })
    ws.push({ type: 'rows', rows: batch })
    ws.push({ type: 'rows', rows: [] })
    ws.push({ type: 'result_end' })
    ws.push({ type: 'statement_start', index: 1, total: 2, start: 10, end: 18, info: { keyword: 'SELECT' } })
    ws.push({ type: 'result_start', columns: [col] })
    ws.push({ type: 'rows', rows: batch })
    await flush()
    expect(ws.sent.map((m) => m.type)).toEqual(['exec', 'fetch_more', 'fetch_more'])
    expect(rt(tabId).exec.waitingForMore).toBe(true)
  })

  it('关闭时关闭会话并丢掉状态', async () => {
    const api = fakeApi()
    const r = new Runner(api, tabId)
    await r.run('select 1', 0)
    await r.dispose(true)
    expect(api.closeSession).toHaveBeenCalledWith(7, true)
    expect(useRuntime.getState().tabs[tabId]).toBeUndefined()
  })
})

describe('偏移换算', () => {
  it('只换带偏移的事件', () => {
    expect(shiftEvent({ type: 'confirm_required', index: 0, start: 1, end: 3, reason: '' }, 10)).toMatchObject({
      start: 11,
      end: 13,
    })
    expect(shiftEvent({ type: 'failed', index: 0, error: { kind: 'x' } }, 10)).not.toHaveProperty('offset')
    expect(shiftEvent({ type: 'done' }, 10)).toEqual({ type: 'done' })
  })

  it('执行单条语句时确认后原样重发', () => {
    const sub = { sql: 'a; b', base: 5, opts: { cursor: 3 } }
    expect(resumeAfterConfirm(sub, 8)).toEqual({ sql: 'a; b', base: 5, opts: { cursor: 3, confirmed: true } })
    expect(resumeAfterConfirm({ sql: 'a; b', base: 5, opts: {} }, 8)).toEqual({
      sql: 'b',
      base: 8,
      opts: { confirmed: true },
    })
  })
})
