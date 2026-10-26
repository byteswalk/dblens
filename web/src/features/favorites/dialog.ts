// 收藏对话框的状态：编辑器（Ctrl+D、工具栏星标）、历史侧栏、收藏侧栏都可以打开它。

import { create } from 'zustand'

import type { DriverId, Favorite } from '../../shared/api'

export type FavoriteDraft =
  /** 收藏一段 SQL。 */
  | { mode: 'create'; kind: 'item'; sql: string; driver?: DriverId; variant?: string | null }
  /** 新建文件夹。 */
  | { mode: 'create'; kind: 'folder'; parentId: string | null }
  /** 编辑收藏或重命名文件夹。 */
  | { mode: 'edit'; fav: Favorite }

/** 设置：上次收藏时选的文件夹，下次默认选它。 */
export const LAST_FOLDER_KEY = 'ui.favorites.last-folder'

interface DialogState {
  draft: FavoriteDraft | null
  open(draft: FavoriteDraft): void
  close(): void
}

export const useFavoriteDialog = create<DialogState>()((set) => ({
  draft: null,
  open: (draft) => set({ draft }),
  close: () => set({ draft: null }),
}))
