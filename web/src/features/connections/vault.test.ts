import { beforeEach, describe, expect, it, vi } from 'vitest'

import { ApiError, type SecretStatus } from '../../shared/api'
import { useVault, withVault } from './vault'

const locked = () => new ApiError(423, { kind: 'locked' })
const api = (initialized: boolean) => ({
  secrets: vi.fn(
    async (): Promise<SecretStatus> => ({ backend: 'vault', vault_initialized: initialized, unlocked: false }),
  ),
})

/** 模拟用户：对话框一弹出就按 ok 关闭。 */
function userAnswers(ok: boolean, seen: string[]) {
  return useVault.subscribe((s) => {
    if (s.pending) {
      seen.push(s.pending.mode)
      queueMicrotask(() => useVault.getState().finish(ok))
    }
  })
}

describe('密码库', () => {
  beforeEach(() => useVault.setState({ pending: null }))

  it('没有锁定时直接返回结果，不打扰用户', async () => {
    const a = api(true)
    expect(await withVault(a, async () => 42)).toBe(42)
    expect(a.secrets).not.toHaveBeenCalled()
  })

  it('锁定时按状态请用户解锁或设置主密码，完成后重试一次', async () => {
    const seen: string[] = []
    const stop = userAnswers(true, seen)
    let calls = 0
    const fn = async () => {
      calls += 1
      if (calls === 1) throw locked()
      return 'ok'
    }
    expect(await withVault(api(true), fn)).toBe('ok')
    calls = 0
    expect(await withVault(api(false), fn)).toBe('ok')
    expect(seen).toEqual(['unlock', 'init'])
    stop()
  })

  it('用户取消时抛出原来的错误；其他错误原样抛出', async () => {
    const stop = userAnswers(false, [])
    await expect(withVault(api(true), async () => Promise.reject(locked()))).rejects.toMatchObject({ status: 423 })
    stop()
    const other = new ApiError(422, { kind: 'connect', message: 'x' })
    await expect(withVault(api(true), async () => Promise.reject(other))).rejects.toBe(other)
  })
})
