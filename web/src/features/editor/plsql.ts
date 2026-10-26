// PL/SQL 高亮（Oracle、达梦共用）：以 Monaco 自带的 sql 规则为基础（M1 设计文档 2.1 节，不从零写），
// 补上 PL/SQL 和 Oracle 常用的关键字，并识别 q'[...]' 字符串。语法分析和补全在 M1b2 用 ANTLR 的 PL/SQL 语法实现。

import type { languages } from 'monaco-editor'
import { conf, language } from 'monaco-editor/languages/definitions/sql/sql'

const EXTRA_KEYWORDS = [
  'BODY', 'BULK', 'COLLECT', 'CONSTANT', 'CONNECT', 'CURSOR', 'DECODE', 'DUAL', 'ELSIF', 'EXCEPTION',
  'EXCEPTIONS', 'EXIT', 'FORALL', 'LEVEL', 'LOOP', 'MINUS', 'NOCOPY', 'NOCYCLE', 'NVL', 'NVL2', 'OTHERS',
  'OUT', 'PACKAGE', 'PIPELINED', 'PIPE', 'PRAGMA', 'PRIOR', 'RAISE', 'RECORD', 'REF', 'RETURNING', 'REVERSE',
  'ROWID', 'ROWNUM', 'ROWTYPE', 'SEQUENCE', 'START', 'SUBTYPE', 'SYNONYM', 'SYSDATE', 'SYSTIMESTAMP', 'TYPE',
  'VARCHAR2', 'NVARCHAR2', 'NUMBER', 'PLS_INTEGER', 'BINARY_INTEGER', 'CLOB', 'NCLOB', 'BLOB', 'BFILE', 'LONG',
  'RAW', 'BOOLEAN', 'TIMESTAMP', 'INTERVAL', 'MERGE', 'MATCHED', 'NOCACHE', 'NOORDER', 'PARTITION', 'SUBPARTITION',
  'TABLESPACE', 'MATERIALIZED', 'PURGE', 'ROLLUP', 'CUBE', 'GROUPING', 'SIBLINGS', 'KEEP', 'DENSE_RANK',
  'LISTAGG', 'WITHIN', 'OVER', 'IMMEDIATE', 'AUTONOMOUS_TRANSACTION', 'DETERMINISTIC', 'RESULT_CACHE',
  'NOTFOUND', 'FOUND', 'ISOPEN', 'ROWCOUNT', 'SQLCODE', 'SQLERRM', 'DBMS_OUTPUT', 'PUT_LINE',
]

export const plsqlConf: languages.LanguageConfiguration = conf

const base = language as languages.IMonarchLanguage & { keywords: string[]; tokenizer: Record<string, unknown[]> }

export const plsqlLanguage: languages.IMonarchLanguage = {
  ...base,
  tokenPostfix: '.plsql',
  keywords: [...new Set([...base.keywords, ...EXTRA_KEYWORDS])],
  tokenizer: {
    ...base.tokenizer,
    // q'[...]'、q'{...}'、q'(...)'、q'<...>' 和 q'!...!' 这样的替代引号字符串
    strings: [
      [/[qQ]'\[/, { token: 'string', next: '@qBracket' }],
      [/[qQ]'\{/, { token: 'string', next: '@qBrace' }],
      [/[qQ]'\(/, { token: 'string', next: '@qParen' }],
      [/[qQ]'</, { token: 'string', next: '@qAngle' }],
      ...(base.tokenizer.strings ?? []),
    ],
    qBracket: [[/\]'/, { token: 'string', next: '@pop' }], [/./, 'string']],
    qBrace: [[/\}'/, { token: 'string', next: '@pop' }], [/./, 'string']],
    qParen: [[/\)'/, { token: 'string', next: '@pop' }], [/./, 'string']],
    qAngle: [[/>'/, { token: 'string', next: '@pop' }], [/./, 'string']],
  } as languages.IMonarchLanguage['tokenizer'],
}
