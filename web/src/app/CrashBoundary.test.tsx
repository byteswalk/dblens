// @vitest-environment jsdom
import { render, screen } from '@testing-library/react'
import { describe, expect, it, vi } from 'vitest'

import { installCrashLog } from '../shared/crash'
import CrashBoundary from './CrashBoundary'

function Boom(): never {
  throw new Error('测试崩溃')
}

describe('崩溃捕获', () => {
  it('显示错误页而不是空白，并上报一次', async () => {
    const send = vi.fn().mockResolvedValue(undefined)
    installCrashLog(send)
    vi.spyOn(console, 'error').mockImplementation(() => {})
    render(
      <CrashBoundary>
        <Boom />
      </CrashBoundary>,
    )
    expect(screen.getByText(/测试崩溃/)).toBeTruthy()
    expect(send).toHaveBeenCalledTimes(1)
    expect(send.mock.calls[0][0]).toMatchObject({ kind: 'render', message: '测试崩溃' })
  })
})
