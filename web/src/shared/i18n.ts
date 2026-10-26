// 界面语言（M1i 设计文档第 3、4 节）：i18next + react-i18next，词条在 src/locales/<语言>/<模块>.json。
// 设置项 ui.lang：auto（跟随浏览器 / 系统，默认）、zh-CN、en。切换后界面立即刷新；
// Monaco 自带的文字（右键菜单、查找框）在加载编辑器之前就要确定，下次启动才换语言。
// 组件里用 useTranslation()；不是组件的模块（菜单、结果整理等）用这里导出的 t。

import i18n from 'i18next'
import { initReactI18next } from 'react-i18next'

import { en, zhCN } from '../locales'

export type Lang = 'zh-CN' | 'en'
export type LangPref = 'auto' | Lang

export const LANG_KEY = 'ui.lang'

export function parseLangPref(raw: string | undefined): LangPref {
  return raw === 'zh-CN' || raw === 'en' ? raw : 'auto'
}

/** `auto` 时取浏览器的首选语言：以 zh 开头的取中文，其余取英文。 */
export function resolveLang(pref: LangPref, languages: readonly string[] = navigator.languages): Lang {
  if (pref !== 'auto') return pref
  return (languages[0] ?? '').toLowerCase().startsWith('zh') ? 'zh-CN' : 'en'
}

void i18n.use(initReactI18next).init({
  resources: { 'zh-CN': { translation: zhCN }, en: { translation: en } },
  lng: resolveLang('auto'),
  fallbackLng: 'zh-CN',
  interpolation: { escapeValue: false },
  initAsync: false,
})

/** 当前界面语言；发给后端的请求带上它，后端按它输出错误和提示。 */
export function currentLang(): Lang {
  return i18n.language === 'en' ? 'en' : 'zh-CN'
}

/** 切换界面语言，并同步页面的 lang 属性。 */
export function applyLang(lang: Lang) {
  document.documentElement.lang = lang
  if (i18n.language !== lang) void i18n.changeLanguage(lang)
}

export const t = i18n.t.bind(i18n)

export default i18n
