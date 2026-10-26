import { describe, expect, it } from 'vitest'

import { desktop, isDesktop, titleBarInset } from './shell'

describe('桌面外壳', () => {
  it('按 Tauri 注入的对象判断是否在桌面程序里', () => {
    expect(isDesktop({})).toBe(false)
    expect(isDesktop({ __TAURI_INTERNALS__: {} })).toBe(true)
  })

  it('测试环境（浏览器模式）下没有桌面外壳', () => {
    expect(desktop).toBeNull()
  })

  it('macOS 给红黄绿按钮留位置', () => {
    expect(titleBarInset('macos')).toBe(76)
    expect(titleBarInset('windows')).toBe(0)
    expect(titleBarInset(undefined)).toBe(0)
  })
})
