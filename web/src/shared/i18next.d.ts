// 词条键的类型：以中文词条为准，写错键名时 tsc 报错。
import 'i18next'

import type { zhCN } from '../locales'

declare module 'i18next' {
  interface CustomTypeOptions {
    resources: { translation: typeof zhCN }
    returnNull: false
  }
}
