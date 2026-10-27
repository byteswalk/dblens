-- PostgreSQL metadata queries (pg_catalog only). Base: PG 10+ (tested on 18.6); version blocks cover prokind (11+) and attgenerated (12+).
-- Booleans are returned as native bool. Internal/temp objects are excluded everywhere.

-- [schemas]
SELECT n.nspname AS name,
       (n.nspname LIKE 'pg!_%' ESCAPE '!' OR n.nspname = 'information_schema') AS is_system
FROM pg_catalog.pg_namespace n
ORDER BY n.nspname

-- [tables]
SELECT c.relname AS name,
       pg_catalog.obj_description(c.oid, 'pg_class') AS comment,
       CASE WHEN c.relkind = 'p' THEN pg_catalog.pg_get_partkeydef(c.oid) END AS partition
FROM pg_catalog.pg_class c
JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace
WHERE n.nspname = {{ bind("schema") }}
  AND c.relkind IN ('r', 'p')
  AND NOT c.relispartition
  AND c.relpersistence <> 't'
{% if table %}
  AND c.relname = {{ bind("table") }}
{% endif %}
{% if filter %}
  AND (UPPER(c.relname) LIKE UPPER({{ bind("filter") }}) ESCAPE '!'
       OR UPPER(pg_catalog.obj_description(c.oid, 'pg_class')) LIKE UPPER({{ bind("filter") }}) ESCAPE '!')
{% endif %}
ORDER BY c.relname

-- [views]
SELECT c.relname AS name,
       pg_catalog.obj_description(c.oid, 'pg_class') AS comment
FROM pg_catalog.pg_class c
JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace
WHERE n.nspname = {{ bind("schema") }}
  AND c.relkind = 'v'
{% if filter %}
  AND (UPPER(c.relname) LIKE UPPER({{ bind("filter") }}) ESCAPE '!'
       OR UPPER(pg_catalog.obj_description(c.oid, 'pg_class')) LIKE UPPER({{ bind("filter") }}) ESCAPE '!')
{% endif %}
ORDER BY c.relname

-- [materialized_views]
SELECT c.relname AS name,
       pg_catalog.obj_description(c.oid, 'pg_class') AS comment
FROM pg_catalog.pg_class c
JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace
WHERE n.nspname = {{ bind("schema") }}
  AND c.relkind = 'm'
{% if filter %}
  AND (UPPER(c.relname) LIKE UPPER({{ bind("filter") }}) ESCAPE '!'
       OR UPPER(pg_catalog.obj_description(c.oid, 'pg_class')) LIKE UPPER({{ bind("filter") }}) ESCAPE '!')
{% endif %}
ORDER BY c.relname

-- [sequences]
-- Sequences owned by a column (serial 'a' / identity 'i') are implementation detail of the column and are hidden.
SELECT c.relname AS name,
       s.seqstart AS start_value,
       s.seqincrement AS increment,
       s.seqmin AS min_value,
       s.seqmax AS max_value,
       s.seqcycle AS cycle,
       s.seqcache AS cache
FROM pg_catalog.pg_class c
JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace
JOIN pg_catalog.pg_sequence s ON s.seqrelid = c.oid
WHERE n.nspname = {{ bind("schema") }}
  AND c.relkind = 'S'
  AND NOT EXISTS (SELECT 1 FROM pg_catalog.pg_depend d
                  WHERE d.classid = 'pg_catalog.pg_class'::regclass AND d.objid = c.oid
                    AND d.refclassid = 'pg_catalog.pg_class'::regclass AND d.deptype IN ('a', 'i'))
{% if filter %}
  AND (UPPER(c.relname) LIKE UPPER({{ bind("filter") }}) ESCAPE '!'
       OR UPPER(pg_catalog.obj_description(c.oid, 'pg_class')) LIKE UPPER({{ bind("filter") }}) ESCAPE '!')
{% endif %}
ORDER BY c.relname

-- [routines]
-- Overloads collapse into ONE row (name = proname); routine_source returns every overload.
SELECT p.proname AS name,
{% if version >= 110000 %}
       CASE p.prokind WHEN 'p' THEN 'PROCEDURE' ELSE 'FUNCTION' END AS kind,
{% else %}
       'FUNCTION' AS kind,
{% endif %}
       MAX(pg_catalog.obj_description(p.oid, 'pg_proc')) AS comment
FROM pg_catalog.pg_proc p
JOIN pg_catalog.pg_namespace n ON n.oid = p.pronamespace
WHERE n.nspname = {{ bind("schema") }}
{% if version >= 110000 %}
  AND p.prokind IN ('f', 'p')
{% else %}
  AND NOT p.proisagg AND NOT p.proiswindow
{% endif %}
{% if filter %}
  AND (UPPER(p.proname) LIKE UPPER({{ bind("filter") }}) ESCAPE '!'
       OR UPPER(pg_catalog.obj_description(p.oid, 'pg_proc')) LIKE UPPER({{ bind("filter") }}) ESCAPE '!')
{% endif %}
GROUP BY p.proname, 2
ORDER BY p.proname, 2

-- [columns]
SELECT c.relname AS table_name,
       a.attname AS name,
       (ROW_NUMBER() OVER (PARTITION BY c.oid ORDER BY a.attnum))::int AS ordinal,
       pg_catalog.format_type(a.atttypid, a.atttypmod) AS type_text,
       NOT a.attnotnull AS nullable,
{% if version >= 120000 %}
       CASE WHEN a.attgenerated = '' THEN pg_catalog.pg_get_expr(d.adbin, d.adrelid) END AS default_value,
{% else %}
       pg_catalog.pg_get_expr(d.adbin, d.adrelid) AS default_value,
{% endif %}
       pg_catalog.col_description(c.oid, a.attnum) AS comment,
       (a.attidentity <> ''
        OR (d.adbin IS NOT NULL AND pg_catalog.pg_get_expr(d.adbin, d.adrelid) LIKE 'nextval(%'
            AND EXISTS (SELECT 1 FROM pg_catalog.pg_depend dp
                        WHERE dp.refclassid = 'pg_catalog.pg_class'::regclass AND dp.refobjid = c.oid
                          AND dp.refobjsubid = a.attnum AND dp.deptype = 'a'
                          AND dp.classid = 'pg_catalog.pg_class'::regclass))) AS auto_increment,
{% if version >= 120000 %}
       CASE WHEN a.attgenerated <> '' THEN pg_catalog.pg_get_expr(d.adbin, d.adrelid) END AS generated
{% else %}
       NULL::text AS generated
{% endif %}
FROM pg_catalog.pg_class c
JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace
JOIN pg_catalog.pg_attribute a ON a.attrelid = c.oid AND a.attnum > 0 AND NOT a.attisdropped
LEFT JOIN pg_catalog.pg_attrdef d ON d.adrelid = a.attrelid AND d.adnum = a.attnum
WHERE n.nspname = {{ bind("schema") }}
  AND c.relkind IN ('r', 'p', 'v', 'm')
  AND NOT c.relispartition
  AND c.relpersistence <> 't'
{% if table %}
  AND c.relname = {{ bind("table") }}
{% endif %}
ORDER BY c.relname, a.attnum

-- [keys]
SELECT c.relname AS table_name,
       con.conname AS constraint_name,
       CASE con.contype WHEN 'p' THEN 'P' WHEN 'u' THEN 'U' ELSE 'F' END AS constraint_type,
       a.attname AS column_name,
       k.pos::int AS position,
       CASE WHEN con.contype = 'f' THEN rn.nspname END AS ref_schema,
       CASE WHEN con.contype = 'f' THEN rc.relname END AS ref_table,
       ra.attname AS ref_column,
       CASE WHEN con.contype = 'f' THEN
         CASE con.confdeltype WHEN 'c' THEN 'CASCADE' WHEN 'n' THEN 'SET NULL' WHEN 'd' THEN 'SET DEFAULT'
                              WHEN 'r' THEN 'RESTRICT' ELSE 'NO ACTION' END END AS on_delete,
       CASE WHEN con.contype = 'f' THEN
         CASE con.confupdtype WHEN 'c' THEN 'CASCADE' WHEN 'n' THEN 'SET NULL' WHEN 'd' THEN 'SET DEFAULT'
                              WHEN 'r' THEN 'RESTRICT' ELSE 'NO ACTION' END END AS on_update
FROM pg_catalog.pg_constraint con
JOIN pg_catalog.pg_class c ON c.oid = con.conrelid
JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace
CROSS JOIN LATERAL pg_catalog.unnest(con.conkey) WITH ORDINALITY AS k(attnum, pos)
JOIN pg_catalog.pg_attribute a ON a.attrelid = con.conrelid AND a.attnum = k.attnum
LEFT JOIN pg_catalog.pg_class rc ON rc.oid = con.confrelid
LEFT JOIN pg_catalog.pg_namespace rn ON rn.oid = rc.relnamespace
LEFT JOIN pg_catalog.pg_attribute ra ON ra.attrelid = con.confrelid AND ra.attnum = con.confkey[k.pos]
WHERE n.nspname = {{ bind("schema") }}
  AND con.contype IN ('p', 'u', 'f')
  AND c.relkind IN ('r', 'p')
  AND NOT c.relispartition
  AND c.relpersistence <> 't'
{% if table %}
  AND c.relname = {{ bind("table") }}
{% endif %}
ORDER BY c.relname, con.conname, k.pos

-- [checks]
-- contype 'c' only: PG 18 not-null constraints are contype 'n' and never match.
SELECT c.relname AS table_name,
       con.conname AS constraint_name,
       pg_catalog.pg_get_expr(con.conbin, con.conrelid) AS expression
FROM pg_catalog.pg_constraint con
JOIN pg_catalog.pg_class c ON c.oid = con.conrelid
JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace
WHERE n.nspname = {{ bind("schema") }}
  AND con.contype = 'c'
  AND c.relkind IN ('r', 'p')
  AND NOT c.relispartition
  AND c.relpersistence <> 't'
{% if table %}
  AND c.relname = {{ bind("table") }}
{% endif %}
ORDER BY c.relname, con.conname

-- [indexes]
-- Key columns only (INCLUDE columns, pos > indnkeyatts, are skipped). Indexes backing PK/UNIQUE constraints are excluded
-- (not those backing FKs: pg_constraint.conindid of an FK points at the *referenced* unique index, so filter contype).
SELECT c.relname AS table_name,
       ic.relname AS index_name,
       i.indisunique AS is_unique,
       UPPER(am.amname) AS method,
       CASE WHEN k.attnum = 0 THEN pg_catalog.pg_get_indexdef(i.indexrelid, k.pos::int, true) ELSE a.attname END AS column_expr,
       k.pos::int AS position,
       ((i.indoption[k.pos - 1] & 1) = 1) AS descending
FROM pg_catalog.pg_index i
JOIN pg_catalog.pg_class c ON c.oid = i.indrelid
JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace
JOIN pg_catalog.pg_class ic ON ic.oid = i.indexrelid
JOIN pg_catalog.pg_am am ON am.oid = ic.relam
CROSS JOIN LATERAL pg_catalog.unnest(i.indkey::int2[]) WITH ORDINALITY AS k(attnum, pos)
LEFT JOIN pg_catalog.pg_attribute a ON a.attrelid = i.indrelid AND a.attnum = k.attnum AND k.attnum > 0
WHERE n.nspname = {{ bind("schema") }}
  AND c.relkind IN ('r', 'p', 'm')
  AND NOT c.relispartition
  AND c.relpersistence <> 't'
  AND k.pos <= i.indnkeyatts
  AND NOT EXISTS (SELECT 1 FROM pg_catalog.pg_constraint cc
                  WHERE cc.conindid = i.indexrelid AND cc.contype IN ('p', 'u') AND cc.conrelid = i.indrelid)
{% if table %}
  AND c.relname = {{ bind("table") }}
{% endif %}
ORDER BY c.relname, ic.relname, k.pos

-- [triggers]
-- tgisinternal excludes FK / constraint-implementation triggers. definition = full CREATE TRIGGER via pg_get_triggerdef.
SELECT c.relname AS table_name,
       t.tgname AS name,
       CASE WHEN (t.tgtype & 64) <> 0 THEN 'INSTEAD OF' WHEN (t.tgtype & 2) <> 0 THEN 'BEFORE' ELSE 'AFTER' END AS timing,
       concat_ws(',',
         CASE WHEN (t.tgtype & 4) <> 0 THEN 'INSERT' END,
         CASE WHEN (t.tgtype & 16) <> 0 THEN 'UPDATE' END,
         CASE WHEN (t.tgtype & 8) <> 0 THEN 'DELETE' END,
         CASE WHEN (t.tgtype & 32) <> 0 THEN 'TRUNCATE' END) AS events,
       pg_catalog.pg_get_triggerdef(t.oid, true) AS definition
FROM pg_catalog.pg_trigger t
JOIN pg_catalog.pg_class c ON c.oid = t.tgrelid
JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace
WHERE n.nspname = {{ bind("schema") }}
  AND NOT t.tgisinternal
  AND c.relkind IN ('r', 'p', 'v')
  AND NOT c.relispartition
  AND c.relpersistence <> 't'
{% if table %}
  AND c.relname = {{ bind("table") }}
{% endif %}
ORDER BY c.relname, t.tgname

-- [view_definition]
SELECT pg_catalog.pg_get_viewdef(c.oid, true) AS definition
FROM pg_catalog.pg_class c
JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace
WHERE n.nspname = {{ bind("schema") }}
  AND c.relname = {{ bind("name") }}
  AND c.relkind IN ('v', 'm')

-- [routine_source]
-- One row per overload (line = 1..n, ordered by argument signature); each text is a full CREATE OR REPLACE
-- followed by an empty line, so the assembler's concatenation yields blank-line-separated overloads.
SELECT (ROW_NUMBER() OVER (ORDER BY pg_catalog.pg_get_function_identity_arguments(p.oid), p.oid))::int AS line,
       pg_catalog.pg_get_functiondef(p.oid) || E'\n' AS text
FROM pg_catalog.pg_proc p
JOIN pg_catalog.pg_namespace n ON n.oid = p.pronamespace
WHERE n.nspname = {{ bind("schema") }}
  AND p.proname = {{ bind("name") }}
{% if version >= 110000 %}
  AND p.prokind IN ('f', 'p')
{% if kind %}
  AND p.prokind = CASE {{ bind("kind") }}::text WHEN 'PROCEDURE' THEN 'p' ELSE 'f' END
{% endif %}
{% else %}
  AND NOT p.proisagg AND NOT p.proiswindow
{% endif %}
ORDER BY line

-- [reserved_words]
-- catcode R = reserved, T = reserved (can be function or type name). C (col_name) and U are not reserved.
SELECT word FROM pg_catalog.pg_get_keywords() WHERE catcode IN ('R', 'T') ORDER BY word

-- [referencing_keys]
-- Table designer rebuild: foreign keys of OTHER tables that reference this table.
SELECT n.nspname AS table_schema,
       c.relname AS table_name,
       con.conname AS constraint_name,
       a.attname AS column_name,
       k.pos::int AS position,
       ra.attname AS ref_column,
       CASE con.confdeltype WHEN 'c' THEN 'CASCADE' WHEN 'n' THEN 'SET NULL' WHEN 'd' THEN 'SET DEFAULT'
                         WHEN 'r' THEN 'RESTRICT' ELSE 'NO ACTION' END AS on_delete,
       CASE con.confupdtype WHEN 'c' THEN 'CASCADE' WHEN 'n' THEN 'SET NULL' WHEN 'd' THEN 'SET DEFAULT'
                         WHEN 'r' THEN 'RESTRICT' ELSE 'NO ACTION' END AS on_update
FROM pg_catalog.pg_constraint con
JOIN pg_catalog.pg_class c ON c.oid = con.conrelid
JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace
JOIN pg_catalog.pg_class t ON t.oid = con.confrelid
JOIN pg_catalog.pg_namespace tn ON tn.oid = t.relnamespace
CROSS JOIN LATERAL pg_catalog.unnest(con.conkey) WITH ORDINALITY AS k(attnum, pos)
JOIN pg_catalog.pg_attribute a ON a.attrelid = con.conrelid AND a.attnum = k.attnum
JOIN pg_catalog.pg_attribute ra ON ra.attrelid = con.confrelid AND ra.attnum = con.confkey[k.pos]
WHERE con.contype = 'f'
  AND tn.nspname = {{ bind("schema") }}
  AND t.relname = {{ bind("table") }}
  AND con.conrelid <> con.confrelid
ORDER BY n.nspname, c.relname, con.conname, k.pos

-- [dependent_views]
-- Views / materialized views whose rewrite rule depends on this table (PG cannot drop the table under them).
SELECT DISTINCT vn.nspname AS schema_name,
       v.relname AS name,
       CASE v.relkind WHEN 'm' THEN 'MATERIALIZED VIEW' ELSE 'VIEW' END AS kind
FROM pg_catalog.pg_depend d
JOIN pg_catalog.pg_rewrite r ON r.oid = d.objid
JOIN pg_catalog.pg_class v ON v.oid = r.ev_class
JOIN pg_catalog.pg_namespace vn ON vn.oid = v.relnamespace
JOIN pg_catalog.pg_class t ON t.oid = d.refobjid
JOIN pg_catalog.pg_namespace tn ON tn.oid = t.relnamespace
WHERE d.classid = 'pg_catalog.pg_rewrite'::regclass
  AND d.refclassid = 'pg_catalog.pg_class'::regclass
  AND tn.nspname = {{ bind("schema") }}
  AND t.relname = {{ bind("table") }}
  AND v.oid <> t.oid
ORDER BY 1, 2

-- [grants]
-- Privileges granted on the table to others (the owner's implicit privileges are skipped). grantee 0 = PUBLIC.
SELECT CASE WHEN a.grantee = 0 THEN 'PUBLIC' ELSE pg_catalog.pg_get_userbyid(a.grantee) END AS grantee,
       a.privilege_type AS privilege,
       a.is_grantable AS grantable
FROM pg_catalog.pg_class c
JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace
CROSS JOIN LATERAL pg_catalog.aclexplode(c.relacl) a
WHERE n.nspname = {{ bind("schema") }}
  AND c.relname = {{ bind("table") }}
  AND a.grantee <> c.relowner
ORDER BY 1, 2

-- [table_storage]
-- reltuples is -1 (PG 14+) or 0 until the table is analyzed.
SELECT NULL::text AS engine,
       NULL::text AS charset,
       NULL::text AS collation,
       ts.spcname AS tablespace,
       c.reltuples::bigint AS row_count
FROM pg_catalog.pg_class c
JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace
LEFT JOIN pg_catalog.pg_tablespace ts ON ts.oid = c.reltablespace
WHERE n.nspname = {{ bind("schema") }}
  AND c.relname = {{ bind("table") }}
  AND c.relkind IN ('r', 'p')
