// 本地密码库（M0-4a）：系统没有钥匙串时，密码用主密码加密后存在本地，需要先设置主密码、每次启动后解锁。
// 任何操作遇到"密码库已锁定"（kind = locked），都先请用户设置或输入主密码，成功后重试一次（M1 设计文档第 5 节）。

import { create } from 'zustand'

import { type Api, ApiError } from '../../shared/api'

export type VaultMode = 'init' | 'unlock'

interface VaultState {
  /** 正在等用户输入主密码的请求。 */
  pending: { mode: VaultMode; resolve: (ok: boolean) => void } | null
  /** 弹出对话框，用户完成（true）或取消（false）后返回。 */
  ask(mode: VaultMode): Promise<boolean>
  finish(ok: boolean): void
}

export const useVault = create<VaultState>()((set, get) => ({
  pending: null,
  ask: (mode) =>
    new Promise<boolean>((resolve) => {
      get().pending?.resolve(false)
      set({ pending: { mode, resolve } })
    }),
  finish: (ok) => {
    get().pending?.resolve(ok)
    set({ pending: null })
  },
}))

export function isLocked(e: unknown): boolean {
  return e instanceof ApiError && e.body.kind === 'locked'
}

/** 执行 fn；遇到密码库锁定时请用户设置或输入主密码，成功后重试一次。用户取消时抛出原来的错误。 */
export async function withVault<T>(api: Pick<Api, 'secrets'>, fn: () => Promise<T>): Promise<T> {
  try {
    return await fn()
  } catch (e) {
    if (!isLocked(e)) throw e
    const status = await api.secrets()
    const ok = await useVault.getState().ask(status.vault_initialized ? 'unlock' : 'init')
    if (!ok) throw e
    return fn()
  }
}
