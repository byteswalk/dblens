// @vitest-environment jsdom
// 连接对话框的关键交互（M1 设计文档第 6 节）：校验不通过时不调接口；测试连接带上表单内容和密码，并显示结果。

import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { cleanup, render, screen, waitFor } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { App as AntApp } from 'antd'
import { afterEach, beforeAll, describe, expect, it, vi } from 'vitest'

import { ApiContext } from '../../app/context'
import { useUi } from '../../app/ui'
import type { Api } from '../../shared/api'
import ConnectionDialog from './ConnectionDialog'

beforeAll(() => {
  // antd 依赖 matchMedia 和 ResizeObserver，jsdom 没有
  globalThis.ResizeObserver = class {
    observe() {}
    unobserve() {}
    disconnect() {}
  }
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

afterEach(() => {
  cleanup()
  useUi.setState({ connectionDialog: null })
})

function setup() {
  const api = {
    drivers: vi.fn(async () => []),
    secrets: vi.fn(),
    testConnection: vi.fn(async () => ({
      product: 'MySQL',
      version: '9.7.4',
      variant: 'mysql',
      variant_name: 'MySQL',
      version_code: 90704,
    })),
    createProfile: vi.fn(),
  } as unknown as Api
  render(
    <ApiContext.Provider value={api}>
      <QueryClientProvider client={new QueryClient()}>
        <AntApp>
          <ConnectionDialog />
        </AntApp>
      </QueryClientProvider>
    </ApiContext.Provider>,
  )
  useUi.setState({ connectionDialog: { profileId: null } })
  return api
}

/** 按表单项的标签找输入框（antd 的 Form.Item 没有把 label 和输入框关联起来）。 */
async function inputOf(label: string): Promise<HTMLInputElement> {
  const l = await screen.findByText(label, { selector: 'label' })
  return l.closest('.ant-form-item')!.querySelector('input')!
}

describe('连接对话框', () => {
  it('必填项没填时保存，显示错误且不调用接口', async () => {
    const api = setup()
    await userEvent.click(await screen.findByRole('button', { name: /保\s*存/ }))
    expect(await screen.findByText('填写连接名称')).toBeTruthy()
    expect(screen.getByText('填写用户名')).toBeTruthy()
    expect(api.createProfile).not.toHaveBeenCalled()
  })

  it('测试连接带上表单内容和密码，显示服务器版本', async () => {
    const api = setup()
    await userEvent.type(await inputOf('名称'), 'local-mysql')
    await userEvent.type(await inputOf('用户'), 'root')
    await userEvent.type(await inputOf('密码'), 'pw')
    await userEvent.click(screen.getByRole('button', { name: /测试连接/ }))
    await waitFor(() => expect(api.testConnection).toHaveBeenCalled())
    expect(api.testConnection).toHaveBeenCalledWith(
      expect.objectContaining({ name: 'local-mysql', driver: 'mysql', host: '127.0.0.1', port: 3306, user: 'root' }),
      'pw',
    )
    expect(await screen.findByText('连接成功：MySQL 9.7.4，识别为 MySQL')).toBeTruthy()
  })
})
