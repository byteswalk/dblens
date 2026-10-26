// 界面设置（主题、面板大小、侧栏等）：启动时从 GET /api/settings 读一次，修改后逐项 PUT 回后端，
// 下次启动时恢复（M1 设计文档 1.1 节）。值都是字符串，结构化的值由使用方自己序列化。

import { create } from 'zustand'

export type SaveSetting = (key: string, value: string | null) => Promise<unknown>

interface SettingsState {
  values: Record<string, string>
  save: SaveSetting | null
  /** 启动时装入后端的设置，并指定以后怎么保存。 */
  init(values: Record<string, string>, save: SaveSetting): void
  /** 修改一项；`null` 删除。立即生效，后台保存，保存失败只在控制台记录（界面设置不值得打断用户）。 */
  set(key: string, value: string | null): void
}

export const useSettings = create<SettingsState>()((set, get) => ({
  values: {},
  save: null,
  init: (values, save) => set({ values, save }),
  set: (key, value) => {
    const values = { ...get().values }
    if (value === null) delete values[key]
    else values[key] = value
    set({ values })
    get()
      .save?.(key, value)
      .catch((e: unknown) => console.warn(`保存设置 ${key} 失败`, e))
  },
}))

/** 读取一项设置（在组件中用，值变化时组件会重新渲染）。 */
export function useSetting(key: string): string | undefined {
  return useSettings((s) => s.values[key])
}

/** 设置项名称只允许小写字母、数字和 `._-`（与后端一致），其他字符换成 `-`。 */
export function settingKey(raw: string): string {
  return raw.toLowerCase().replace(/[^a-z0-9._-]/g, '-').slice(0, 64)
}

/**
 * 给 react-resizable-panels 的 useDefaultLayout 用的存储：面板布局存进设置，键为 `ui.layout.<名称>`。
 */
export const layoutStorage = {
  getItem: (name: string): string | null => useSettings.getState().values[settingKey(`ui.layout.${name}`)] ?? null,
  setItem: (name: string, value: string): void => useSettings.getState().set(settingKey(`ui.layout.${name}`), value),
}
