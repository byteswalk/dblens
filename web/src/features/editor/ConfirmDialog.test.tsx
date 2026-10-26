// @vitest-environment jsdom
// 生产确认框的流程（M1 设计文档第 6 节）：显示语句和原因，"取消"和"仍然执行"各自回调。

import { cleanup, render, screen } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { afterEach, beforeAll, describe, expect, it, vi } from 'vitest'

import i18n from '../../shared/i18n'
import ConfirmDialog from './ConfirmDialog'

beforeAll(() => {
  // antd 依赖 matchMedia，jsdom 没有
  window.matchMedia = vi.fn().mockImplementation((query: string) => ({
    matches: false,
    media: query,
    addEventListener: vi.fn(),
    removeEventListener: vi.fn(),
    addListener: vi.fn(),
    removeListener: vi.fn(),
    onchange: null,
    dispatchEvent: vi.fn(),
  }))
})

afterEach(cleanup)

describe('生产确认框', () => {
  it('显示语句和原因，"取消"和"仍然执行"各自回调', async () => {
    const onConfirm = vi.fn()
    const onCancel = vi.fn()
    render(
      <ConfirmDialog
        open
        sql="DELETE FROM orders"
        reason="生产库上不带 WHERE 的 DELETE"
        onConfirm={onConfirm}
        onCancel={onCancel}
      />,
    )
    expect(screen.getByText('DELETE FROM orders')).toBeTruthy()
    expect(screen.getByText('生产库上不带 WHERE 的 DELETE')).toBeTruthy()
    await userEvent.click(screen.getByRole('button', { name: /取\s*消/ }))
    expect(onCancel).toHaveBeenCalledTimes(1)
    await userEvent.click(screen.getByRole('button', { name: '仍然执行' }))
    expect(onConfirm).toHaveBeenCalledTimes(1)
  })

  it('英文界面显示英文按钮', async () => {
    await i18n.changeLanguage('en')
    try {
      render(<ConfirmDialog open sql="DELETE FROM orders" reason="x" onConfirm={vi.fn()} onCancel={vi.fn()} />)
      expect(screen.getByText('Confirm Execution')).toBeTruthy()
      expect(screen.getByRole('button', { name: 'Run Anyway' })).toBeTruthy()
    } finally {
      await i18n.changeLanguage('zh-CN')
    }
  })
})
