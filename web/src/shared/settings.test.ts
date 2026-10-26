import { beforeEach, describe, expect, it, vi } from 'vitest'

import { layoutStorage, settingKey, useSettings } from './settings'

describe('设置', () => {
  beforeEach(() => useSettings.setState({ values: {}, save: null }))

  it('修改立即生效并保存到后端，null 删除', () => {
    const save = vi.fn(() => Promise.resolve())
    useSettings.getState().init({ 'ui.theme': 'dark' }, save)
    useSettings.getState().set('ui.side', 'db')
    useSettings.getState().set('ui.theme', null)
    expect(useSettings.getState().values).toEqual({ 'ui.side': 'db' })
    expect(save.mock.calls).toEqual([
      ['ui.side', 'db'],
      ['ui.theme', null],
    ])
  })

  it('保存失败不抛出，界面状态保留', async () => {
    const warn = vi.spyOn(console, 'warn').mockImplementation(() => {})
    useSettings.getState().init({}, () => Promise.reject(new Error('网络错误')))
    useSettings.getState().set('ui.side', 'db')
    await Promise.resolve()
    await Promise.resolve()
    expect(useSettings.getState().values['ui.side']).toBe('db')
    expect(warn).toHaveBeenCalled()
    warn.mockRestore()
  })

  it('面板布局的键转换成后端允许的设置名', () => {
    expect(settingKey('ui.layout.react-resizable-panels:Main')).toBe('ui.layout.react-resizable-panels-main')
    const save = vi.fn(() => Promise.resolve())
    useSettings.getState().init({}, save)
    layoutStorage.setItem('react-resizable-panels:main', '{"side":20}')
    expect(layoutStorage.getItem('react-resizable-panels:main')).toBe('{"side":20}')
    expect(save).toHaveBeenCalledWith('ui.layout.react-resizable-panels-main', '{"side":20}')
  })
})
