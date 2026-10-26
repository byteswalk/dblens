// monaco-editor 自带的 sql 高亮规则没有类型声明。
declare module 'monaco-editor/languages/definitions/sql/sql' {
  import type { languages } from 'monaco-editor'

  export const conf: languages.LanguageConfiguration
  export const language: languages.IMonarchLanguage
}
