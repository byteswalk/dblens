import { describe, expect, it } from 'vitest'

import { plsql } from './parser'

/** `|` 表示光标。 */
function at(text: string) {
  const offset = text.indexOf('|')
  return plsql.suggest(text.replace('|', ''), offset)
}

describe('PL/SQL 语法检查', () => {
  it('正确的 Oracle 语句没有问题', () => {
    const sql = `SELECT e.id, d.name FROM hr.emp e JOIN hr.dept d ON d.id = e.dept_id WHERE e.salary > 100 FETCH FIRST 10 ROWS ONLY;
MERGE INTO t USING (SELECT id FROM s) x ON (t.id = x.id) WHEN MATCHED THEN UPDATE SET t.v = 1;
CREATE OR REPLACE PACKAGE BODY p AS
  PROCEDURE x IS
  BEGIN
    FOR r IN (SELECT id FROM t) LOOP
      dbms_output.put_line(r.id);
    END LOOP;
  END x;
END p;
/
SELECT 1 FROM dual`
    expect(plsql.validate(sql, 'zh_CN')).toEqual([])
  })

  // 每条补丁（web/scripts/gen-plsql.mjs 的 PATCHES）对应一条语句
  it.each([
    'DROP FUNCTION IF EXISTS f',
    'DROP PACKAGE IF EXISTS p',
    'DROP PROCEDURE IF EXISTS p',
    'DROP TRIGGER IF EXISTS t',
    'DROP SEQUENCE IF EXISTS s',
    'DROP MATERIALIZED VIEW IF EXISTS mv',
    'DROP TABLE IF EXISTS t',
    'DROP VIEW IF EXISTS v',
    'DROP SYNONYM IF EXISTS s',
    'DROP TABLE t CASCADE',
    'CREATE TABLE t (id INT IDENTITY(1,1) NOT NULL, name VARCHAR(50 CHAR))',
    "COMMENT ON VIEW v IS '视图'",
    'CREATE TABLE t (a BIGINT, b TINYINT, c BYTE, d TEXT, e TIME)',
    'CREATE TABLE t (a VARBINARY(100), b NVARCHAR(20))',
    "CREATE TABLE t (id INT, d DATE) PARTITION BY RANGE (d) (PARTITION p1 VALUES LESS THAN (DATE '2024-01-01'))",
  ])('达梦语法补丁：%s', (sql) => {
    expect(plsql.validate(sql, 'zh_CN')).toEqual([])
  })

  it('标出出错的词；语句不完整时标出最后一个词', () => {
    expect(plsql.validate('SELECT * FORM t', 'zh_CN')).toEqual([
      { startLine: 1, startColumn: 10, endLine: 1, endColumn: 14, near: 'FORM' },
    ])
    expect(plsql.validate('SELECT * FROM t WHERE a =', 'zh_CN')).toEqual([
      { startLine: 1, startColumn: 25, endLine: 1, endColumn: 26, incomplete: true },
    ])
  })

  it('逐句检查，每句只报第一处；位置换算成全文中的行列', () => {
    const issues = plsql.validate('SELECT * FORM t WHERE WHERE;\nSELECT 1 FROM dual;\n  SELEC 2 FROM dual', 'zh_CN')
    expect(issues).toEqual([
      { startLine: 1, startColumn: 10, endLine: 1, endColumn: 14, near: 'FORM' },
      // SELEC 被当作过程名，报在后面的 2 上
      { startLine: 3, startColumn: 9, endLine: 3, endColumn: 10, near: '2' },
    ])
  })

  it('PL/SQL 块中的分号不分句，块以单独一行的 / 结束', () => {
    const sql = 'BEGIN\n  NULL;\n  NULL;\nEND;\n/\nSELECT 1 FROM dual;\nSELECT x FORM dual'
    expect(plsql.validate(sql, 'zh_CN').map((i) => i.startLine)).toEqual([7])
  })

  it('字符串没有结束是词法错误', () => {
    const issues = plsql.validate("SELECT 'abc FROM dual", 'zh_CN')
    expect(issues[0]).toMatchObject({ startLine: 1, startColumn: 8 })
  })
})

describe('PL/SQL 补全分析', () => {
  it.each([
    ['SELECT * FROM |', ['table', 'view']],
    ['SELECT * FROM t1, |', ['table', 'view']],
    ['UPDATE |', ['table', 'view']],
    ['DELETE FROM |', ['table', 'view']],
    ['MERGE INTO |', ['table', 'view']],
    ['SELECT * FROM a; SELECT * FROM |', ['table', 'view']],
    ['SELECT x FROM t WHERE x IN (SELECT y FROM |', ['table', 'view']],
    ['BEGIN UPDATE |', ['table', 'view']],
  ])('%s 需要表', (text, want) => {
    expect(at(text).want).toEqual(expect.arrayContaining(want))
  })

  it.each(['SELECT * FROM emp e WHERE |', 'SELECT a FROM t ORDER BY |', 'SELECT |', 'SELECT * FROM emp e JOIN dept d ON |'])(
    '%s 需要列和函数',
    (text) => {
      expect(at(text).want).toEqual(expect.arrayContaining(['column', 'function']))
    },
  )

  it('UPDATE … SET 需要列', () => {
    expect(at('UPDATE emp SET |').want).toContain('column')
  })

  it('限定名与语句中的表和别名', () => {
    const s = at('SELECT e.| FROM hr.employees e JOIN dept d ON d.id = e.dept_id')
    expect(s.qualifier).toEqual(['e'])
    expect(s.tables).toEqual([
      { schema: 'hr', name: 'employees', alias: 'e' },
      { name: 'dept', alias: 'd' },
    ])
    expect(at('SELECT * FROM hr.|').qualifier).toEqual(['hr'])
    expect(at('SELECT * FROM emp WHERE emp.na|').qualifier).toEqual(['emp'])
  })

  it('子查询中也能看到外层的表；只看光标所在的语句', () => {
    const s = at('SELECT 1 FROM other o;\nSELECT * FROM emp e WHERE EXISTS (SELECT 1 FROM dept d WHERE d.id = |)')
    expect(s.tables.map((t) => t.alias)).toEqual(['e', 'd'])
  })

  it('双引号名称去掉引号', () => {
    expect(at('SELECT * FROM "HR"."Emp Tab" x WHERE x.|').tables).toEqual([{ schema: 'HR', name: 'Emp Tab', alias: 'x' }])
  })

  it('关键字只给语法允许的', () => {
    const kw = at('SELECT * FROM emp |').keywords
    expect(kw).toEqual(expect.arrayContaining(['WHERE', 'JOIN', 'ORDER']))
    expect(kw).not.toContain('SELECT')
  })
})
