// 外壳的界面状态：当前侧栏、选中的连接、打开的对话框。侧栏选择存进设置（ui.side），下次启动恢复。

import { create } from 'zustand'

import { useSettings } from '../shared/settings'

export type SideKey = 'db' | 'history' | 'favorites' | 'ops'

export const SIDE_KEY = 'ui.side'

export function parseSide(v: string | undefined): SideKey | null {
  if (v === 'none') return null
  return v === 'history' || v === 'favorites' || v === 'ops' ? v : 'db'
}

interface UiState {
  /** 当前侧栏；null 表示折叠。 */
  side: SideKey | null
  /** 对象树中选中的连接，状态栏显示它。 */
  selectedProfile: string | null
  /** 连接对话框：打开时 profileId 为 null 表示新建。 */
  connectionDialog: { profileId: string | null } | null
  settingsOpen: boolean
  /** 要刷新的对象树节点（导入建表、执行 SQL 文件之后）：schema 为 null 时刷新整个连接；n 每次加一。 */
  treeRefresh: { profileId: string; schema: string | null; n: number } | null
  setSide(side: SideKey | null): void
  /** 点活动栏：点当前侧栏则折叠，否则切换过去。 */
  toggleSide(side: SideKey): void
  selectProfile(id: string | null): void
  openConnection(profileId: string | null): void
  closeConnection(): void
  setSettingsOpen(open: boolean): void
  refreshTree(profileId: string, schema: string | null): void
}

export const useUi = create<UiState>()((set, get) => ({
  side: 'db',
  selectedProfile: null,
  connectionDialog: null,
  settingsOpen: false,
  treeRefresh: null,
  setSide: (side) => {
    set({ side })
    useSettings.getState().set(SIDE_KEY, side ?? 'none')
  },
  toggleSide: (side) => get().setSide(get().side === side ? null : side),
  selectProfile: (selectedProfile) => set({ selectedProfile }),
  openConnection: (profileId) => set({ connectionDialog: { profileId } }),
  closeConnection: () => set({ connectionDialog: null }),
  setSettingsOpen: (settingsOpen) => set({ settingsOpen }),
  refreshTree: (profileId, schema) => set({ treeRefresh: { profileId, schema, n: (get().treeRefresh?.n ?? 0) + 1 } }),
}))
