import { describe, expect, it } from 'vitest'

import { editorLanguage, formatSql, formatterLanguage } from './dialect'

describe('方言对应', () => {
  it('编辑器语言按驱动', () => {
    expect(editorLanguage('mysql')).toBe('mysql')
    expect(editorLanguage('postgres')).toBe('pgsql')
    expect(editorLanguage('oracle')).toBe('plsql')
    expect(editorLanguage('dameng')).toBe('plsql')
  })

  it('格式化方言优先按变体', () => {
    expect(formatterLanguage('mysql')).toBe('mysql')
    expect(formatterLanguage('mysql', 'mariadb')).toBe('mariadb')
    expect(formatterLanguage('mysql', 'tidb')).toBe('tidb')
    expect(formatterLanguage('mysql', 'polardb')).toBe('mysql')
    expect(formatterLanguage('postgres', 'kingbase')).toBe('postgresql')
    expect(formatterLanguage('dameng', 'dameng')).toBe('plsql')
  })

  it('格式化保留关键字大小写，多条语句之间空一行', () => {
    expect(formatSql('select a,b from t where id=1;select 2', 'postgres')).toBe(
      'select\n  a,\n  b\nfrom\n  t\nwhere\n  id = 1;\n\nselect\n  2',
    )
    // PG 的 $$ 和 Oracle 的 q'' 字符串不被拆开
    expect(formatSql("select $$a;b$$", 'postgres')).toContain('$$a;b$$')
    expect(formatSql("select q'[a;b]' from dual", 'oracle')).toContain("q'[a;b]'")
  })
})
