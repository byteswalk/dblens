// 导入向导、执行 SQL 文件对话框的打开状态：对象树、标题栏菜单、数据标签都可以打开它们。

import { create } from 'zustand'

export interface ImportTarget {
  profileId: string
  /** 入口所在的 schema；null 时由用户选择。 */
  schema: string | null
  /** 导入到这张已有表；null 时由用户选择或新建。 */
  table: string | null
  /** 打开时默认导入到新表。 */
  create: boolean
}

export interface SqlFileTarget {
  profileId: string
  schema: string | null
}

interface DialogState {
  importTarget: ImportTarget | null
  sqlTarget: SqlFileTarget | null
  openImport(target: ImportTarget): void
  openSqlFile(target: SqlFileTarget): void
  closeImport(): void
  closeSqlFile(): void
}

export const useImportDialogs = create<DialogState>()((set) => ({
  importTarget: null,
  sqlTarget: null,
  openImport: (importTarget) => set({ importTarget }),
  openSqlFile: (sqlTarget) => set({ sqlTarget }),
  closeImport: () => set({ importTarget: null }),
  closeSqlFile: () => set({ sqlTarget: null }),
}))
