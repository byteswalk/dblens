import { describe, expect, it } from 'vitest'

import { tsql } from './parser'

/** `|` 表示光标。 */
function at(text: string) {
  const offset = text.indexOf('|')
  return tsql.suggest(text.replace('|', ''), offset)
}

describe('T-SQL 语法检查', () => {
  it('常见的 T-SQL 批没有问题', () => {
    const sql = `SELECT TOP (10) e.id, d.[name] FROM dbo.emp AS e JOIN [dbo].[dept] d ON d.id = e.dept_id WHERE e.salary > 100 ORDER BY e.id
GO
DECLARE @n INT = 5
INSERT INTO #t (id) VALUES (@n)
UPDATE t SET v = 1 OUTPUT inserted.id WHERE id = @n
GO 3
CREATE PROCEDURE dbo.p @a INT AS
BEGIN
  SET NOCOUNT ON;
  IF @a > 0 SELECT 1 ELSE PRINT N'负数'
END
GO
SELECT id FROM t ORDER BY id OFFSET 10 ROWS FETCH NEXT 5 ROWS ONLY;
MERGE INTO t USING s ON t.id = s.id WHEN MATCHED THEN UPDATE SET t.v = s.v;`
    expect(tsql.validate(sql, 'zh_CN')).toEqual([])
  })

  it('标出出错的位置；位置换算成全文中的行列', () => {
    const issues = tsql.validate('SELECT 1\nGO\nSELECT * FROM t WHERE', 'zh_CN')
    expect(issues).toHaveLength(1)
    expect(issues[0]).toMatchObject({ startLine: 3, incomplete: true })
    expect(tsql.validate('SELECT 1\nSELECT * FROM t WHERE a = = 1', 'zh_CN')[0]).toMatchObject({ startLine: 2, near: '=' })
  })
})

describe('T-SQL 分批', () => {
  it('单独一行的 GO 分批，字符串、注释、方括号里的 GO 不算', () => {
    const sql = "SELECT 'GO'\n/* GO */\nSELECT [GO]\n  go 2 -- 两次\nSELECT 3\nGOTO x"
    const second = sql.indexOf('SELECT 3')
    expect(tsql.statementAt(sql, 0)).toEqual({ start: 0, end: sql.indexOf('[GO]') + 4 })
    expect(tsql.statementAt(sql, second + 2)).toEqual({ start: second, end: sql.length })
  })

  it('一批里的多条语句是同一个执行单位', () => {
    const sql = 'SELECT 1\nSELECT 2\nGO\nSELECT 3'
    expect(tsql.statementAt(sql, sql.indexOf('2'))).toEqual({ start: 0, end: sql.indexOf('\nGO') })
  })
})

describe('T-SQL 补全', () => {
  it('FROM 后要表，表名前的限定名、三段名、别名', () => {
    const s = at('SELECT * FROM |')
    expect(s.want).toContain('table')
    const q = at('SELECT * FROM dbo.|')
    expect(q.qualifier).toEqual(['dbo'])
    const t = at('SELECT e.| FROM sales.dbo.emp e JOIN [hr].[dept] AS d ON 1 = 1')
    expect(t.qualifier).toEqual(['e'])
    expect(t.tables).toEqual([
      { schema: 'dbo', name: 'emp', alias: 'e' },
      { schema: 'hr', name: 'dept', alias: 'd' },
    ])
  })

  it('语句开头给关键字', () => {
    expect(at('|').keywords).toEqual(expect.arrayContaining(['SELECT', 'INSERT', 'DECLARE']))
  })
})
