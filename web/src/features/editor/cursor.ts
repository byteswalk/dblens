// 编辑器光标位置，状态栏显示。单独放一个文件，状态栏不必引入 Monaco。

import { create } from 'zustand'

export const useCursor = create<{ line: number; column: number }>()(() => ({ line: 1, column: 1 }))
