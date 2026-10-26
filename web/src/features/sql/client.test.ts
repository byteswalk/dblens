import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

import { SqlClient, TIMEOUT_MS, type WorkerLike } from './client'
import type { Request, Response } from './protocol'

/** 假 worker：记下收到的请求，由测试决定怎么回复。 */
class FakeWorker implements WorkerLike {
  sent: Request[] = []
  onmessage: ((e: MessageEvent<Response>) => void) | null = null
  onerror: ((e: unknown) => void) | null = null
  postMessage(msg: Request) {
    this.sent.push(msg)
    if (msg.op === 'load') this.reply({ id: msg.id, ok: true, result: null })
  }
  reply(r: Response) {
    queueMicrotask(() => this.onmessage?.({ data: r } as MessageEvent<Response>))
  }
}

describe('SqlClient', () => {
  let w: FakeWorker
  let client: SqlClient
  beforeEach(() => {
    vi.useFakeTimers()
    vi.spyOn(console, 'warn').mockImplementation(() => {})
    w = new FakeWorker()
    client = new SqlClient(() => w)
  })
  afterEach(() => {
    vi.useRealTimers()
    vi.restoreAllMocks()
  })

  it('先加载解析器，再按序号返回结果', async () => {
    const p = client.validate('mysql', 'SELECT', 'zh_CN')
    await vi.waitFor(() => expect(w.sent).toHaveLength(2))
    expect(w.sent.map((r) => r.op)).toEqual(['load', 'validate'])
    const issues = [{ startLine: 1, startColumn: 1, endLine: 1, endColumn: 7, incomplete: true }]
    w.reply({ id: w.sent[1].id, ok: true, result: issues })
    await expect(p).resolves.toEqual({ ok: true, value: issues })
    // 同一方言不再加载
    void client.validate('mysql', 'SELECT 1', 'zh_CN')
    await vi.waitFor(() => expect(w.sent).toHaveLength(3))
    expect(w.sent[2].op).toBe('validate')
  })

  it('超过 2 秒放弃结果', async () => {
    const p = client.suggest('postgres', 'SELECT ', 7)
    await vi.waitFor(() => expect(w.sent).toHaveLength(2))
    vi.advanceTimersByTime(TIMEOUT_MS + 1)
    await expect(p).resolves.toEqual({ ok: false, reason: 'timeout' })
  })

  it('解析器加载失败后该方言不再使用', async () => {
    w.postMessage = (msg) => {
      w.sent.push(msg)
      w.reply({ id: msg.id, ok: false, error: 'chunk load failed' })
    }
    await expect(client.validate('plsql', 'x', 'zh_CN')).resolves.toEqual({ ok: false, reason: 'unavailable' })
    expect(client.unavailable('plsql')).toBe(true)
    expect(client.unavailable('mysql')).toBe(false)
    await client.validate('plsql', 'y', 'zh_CN')
    expect(w.sent).toHaveLength(1)
  })

  it('worker 出错后全部不可用，等待中的请求立即结束', async () => {
    const p = client.validate('mysql', 'SELECT', 'zh_CN')
    await vi.waitFor(() => expect(w.sent).toHaveLength(2))
    w.onerror?.(new Error('boom'))
    await expect(p).resolves.toEqual({ ok: false, reason: 'unavailable' })
    expect(client.unavailable('postgres')).toBe(true)
  })
})
