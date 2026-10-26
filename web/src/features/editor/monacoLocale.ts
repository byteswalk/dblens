// Monaco 自带文字（右键菜单、查找框等）的语言：必须在 monaco-editor 加载之前设定，
// 所以由外壳在懒加载工作区之前调用（见 app/Shell.tsx）。英文是 Monaco 的默认语言，不用加载。

import { currentLang } from '../../shared/i18n'

export async function loadMonacoLocale() {
  if (currentLang() === 'zh-CN') await import('monaco-editor/nls/lang/zh-cn.js')
}
