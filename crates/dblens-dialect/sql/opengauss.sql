-- openGauss overrides on top of postgres.sql (only keys that differ). Based on PG 9.2; tested on 7.0.0-RC1
-- with databases in A (Oracle-compatible) and PG compatibility. Compared with postgres.sql: no LATERAL /
-- WITH ORDINALITY, no relispartition (partitions are in pg_partition), no pg_sequence, pg_get_partkeydef,
-- attidentity or attgenerated.

-- [variant_probe]
-- Compatibility of the current database (A / B / C / PG) and the PostgreSQL version the build is based on.
SELECT datcompatibility AS mode,
       current_setting('server_version_num') AS version_num
FROM pg_catalog.pg_database
WHERE datname = current_database()

-- [schemas]
-- Built-in schemas besides pg_* and information_schema (dbe_*, pkg_*, db4ai, ...).
SELECT n.nspname AS name,
       (n.nspname LIKE 'pg!_%' ESCAPE '!' OR n.nspname LIKE 'dbe!_%' ESCAPE '!' OR n.nspname LIKE 'pkg!_%' ESCAPE '!'
        OR n.nspname IN ('information_schema', 'blockchain', 'coverage', 'cstore', 'db4ai', 'snapshot',
                         'sqladvisor', 'xmltype')) AS is_system
FROM pg_catalog.pg_namespace n
ORDER BY n.nspname

-- [tables]
-- Partitions live in pg_partition, not pg_class: a partitioned table is relkind r with parttype p.
SELECT c.relname AS name,
       pg_catalog.obj_description(c.oid, 'pg_class') AS comment,
       CASE WHEN c.parttype = 'p' THEN
         (SELECT CASE p.partstrategy WHEN 'r' THEN 'RANGE' WHEN 'l' THEN 'LIST' WHEN 'h' THEN 'HASH'
                                     WHEN 'i' THEN 'INTERVAL' ELSE 'UNKNOWN' END
                 || ' (' || (SELECT pg_catalog.string_agg(a.attname, ', ' ORDER BY a.attnum)
                               FROM pg_catalog.pg_attribute a
                              WHERE a.attrelid = c.oid AND a.attnum = ANY (p.partkey::int2[])) || ')'
            FROM pg_catalog.pg_partition p
           WHERE p.parentid = c.oid AND p.parttype = 'r')
       END AS partition
FROM pg_catalog.pg_class c
JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace
WHERE n.nspname = {{ bind("schema") }}
  AND c.relkind IN ('r', 'p')
  AND c.relpersistence <> 't'
{% if table %}
  AND c.relname = {{ bind("table") }}
{% endif %}
{% if filter %}
  AND (UPPER(c.relname) LIKE UPPER({{ bind("filter") }}) ESCAPE '!'
       OR UPPER(pg_catalog.obj_description(c.oid, 'pg_class')) LIKE UPPER({{ bind("filter") }}) ESCAPE '!')
{% endif %}
ORDER BY c.relname

-- [sequences]
-- No pg_sequence: parameters come from pg_sequence_parameters, which has no cache size.
SELECT c.relname AS name,
       (pg_catalog.pg_sequence_parameters(c.oid)).start_value AS start_value,
       (pg_catalog.pg_sequence_parameters(c.oid)).increment AS increment,
       (pg_catalog.pg_sequence_parameters(c.oid)).minimum_value AS min_value,
       (pg_catalog.pg_sequence_parameters(c.oid)).maximum_value AS max_value,
       (pg_catalog.pg_sequence_parameters(c.oid)).cycle_option AS cycle,
       NULL::bigint AS cache
FROM pg_catalog.pg_class c
JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace
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

-- [columns]
-- openGauss has serial only; GaussDB also has identity columns, whose sequence is named <table>_<column>_identity and
-- has no 'a' dependency. A generated column has pg_attrdef.adgencol = 's'.
SELECT c.relname AS table_name,
       a.attname AS name,
       (ROW_NUMBER() OVER (PARTITION BY c.oid ORDER BY a.attnum))::int AS ordinal,
       pg_catalog.format_type(a.atttypid, a.atttypmod) AS type_text,
       NOT a.attnotnull AS nullable,
       CASE WHEN d.adgencol = 's' THEN NULL ELSE pg_catalog.pg_get_expr(d.adbin, d.adrelid) END AS default_value,
       pg_catalog.col_description(c.oid, a.attnum) AS comment,
       ((d.adbin IS NOT NULL AND pg_catalog.pg_get_expr(d.adbin, d.adrelid) LIKE 'nextval(%'
            AND (pg_catalog.pg_get_expr(d.adbin, d.adrelid) LIKE '%!_identity''::regclass)' ESCAPE '!'
                 OR EXISTS (SELECT 1 FROM pg_catalog.pg_depend dp
                            WHERE dp.refclassid = 'pg_catalog.pg_class'::regclass AND dp.refobjid = c.oid
                              AND dp.refobjsubid = a.attnum AND dp.deptype = 'a'
                              AND dp.classid = 'pg_catalog.pg_class'::regclass)))) AS auto_increment,
       CASE WHEN d.adgencol = 's' THEN pg_catalog.pg_get_expr(d.adbin, d.adrelid) END AS generated
FROM pg_catalog.pg_class c
JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace
JOIN pg_catalog.pg_attribute a ON a.attrelid = c.oid AND a.attnum > 0 AND NOT a.attisdropped
LEFT JOIN pg_catalog.pg_attrdef d ON d.adrelid = a.attrelid AND d.adnum = a.attnum
WHERE n.nspname = {{ bind("schema") }}
  AND c.relkind IN ('r', 'p', 'v', 'm')
  AND c.relpersistence <> 't'
{% if table %}
  AND c.relname = {{ bind("table") }}
{% endif %}
ORDER BY c.relname, a.attnum

-- [keys]
-- No LATERAL / WITH ORDINALITY: key positions come from generate_series in a subquery.
SELECT c.relname AS table_name,
       con.conname AS constraint_name,
       CASE con.contype WHEN 'p' THEN 'P' WHEN 'u' THEN 'U' ELSE 'F' END AS constraint_type,
       a.attname AS column_name,
       con.pos::int AS position,
       CASE WHEN con.contype = 'f' THEN rn.nspname END AS ref_schema,
       CASE WHEN con.contype = 'f' THEN rc.relname END AS ref_table,
       ra.attname AS ref_column,
       CASE WHEN con.contype = 'f' THEN
         CASE con.confdeltype WHEN 'c' THEN 'CASCADE' WHEN 'n' THEN 'SET NULL' WHEN 'd' THEN 'SET DEFAULT'
                              WHEN 'r' THEN 'RESTRICT' ELSE 'NO ACTION' END END AS on_delete,
       CASE WHEN con.contype = 'f' THEN
         CASE con.confupdtype WHEN 'c' THEN 'CASCADE' WHEN 'n' THEN 'SET NULL' WHEN 'd' THEN 'SET DEFAULT'
                              WHEN 'r' THEN 'RESTRICT' ELSE 'NO ACTION' END END AS on_update
FROM (SELECT x.*, pg_catalog.generate_series(1, pg_catalog.array_length(x.conkey, 1)) AS pos
        FROM pg_catalog.pg_constraint x) con
JOIN pg_catalog.pg_class c ON c.oid = con.conrelid
JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace
JOIN pg_catalog.pg_attribute a ON a.attrelid = con.conrelid AND a.attnum = con.conkey[con.pos]
LEFT JOIN pg_catalog.pg_class rc ON rc.oid = con.confrelid
LEFT JOIN pg_catalog.pg_namespace rn ON rn.oid = rc.relnamespace
LEFT JOIN pg_catalog.pg_attribute ra ON ra.attrelid = con.confrelid AND ra.attnum = con.confkey[con.pos]
WHERE n.nspname = {{ bind("schema") }}
  AND con.contype IN ('p', 'u', 'f')
  AND c.relkind IN ('r', 'p')
  AND c.relpersistence <> 't'
{% if table %}
  AND c.relname = {{ bind("table") }}
{% endif %}
ORDER BY c.relname, con.conname, con.pos

-- [checks]
SELECT c.relname AS table_name,
       con.conname AS constraint_name,
       pg_catalog.pg_get_expr(con.conbin, con.conrelid) AS expression
FROM pg_catalog.pg_constraint con
JOIN pg_catalog.pg_class c ON c.oid = con.conrelid
JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace
WHERE n.nspname = {{ bind("schema") }}
  AND con.contype = 'c'
  AND c.relkind IN ('r', 'p')
  AND c.relpersistence <> 't'
{% if table %}
  AND c.relname = {{ bind("table") }}
{% endif %}
ORDER BY c.relname, con.conname

-- [indexes]
-- No LATERAL / WITH ORDINALITY: key positions come from generate_series over indnkeyatts.
SELECT c.relname AS table_name,
       ic.relname AS index_name,
       i.indisunique AS is_unique,
       UPPER(am.amname) AS method,
       CASE WHEN i.indkey[i.pos - 1] = 0 THEN pg_catalog.pg_get_indexdef(i.indexrelid, i.pos::int, true) ELSE a.attname END AS column_expr,
       i.pos::int AS position,
       ((i.indoption[i.pos - 1] & 1) = 1) AS descending
FROM (SELECT x.*, pg_catalog.generate_series(1, x.indnkeyatts) AS pos FROM pg_catalog.pg_index x) i
JOIN pg_catalog.pg_class c ON c.oid = i.indrelid
JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace
JOIN pg_catalog.pg_class ic ON ic.oid = i.indexrelid
JOIN pg_catalog.pg_am am ON am.oid = ic.relam
LEFT JOIN pg_catalog.pg_attribute a ON a.attrelid = i.indrelid AND a.attnum = i.indkey[i.pos - 1] AND i.indkey[i.pos - 1] > 0
WHERE n.nspname = {{ bind("schema") }}
  AND c.relkind IN ('r', 'p', 'm')
  AND c.relpersistence <> 't'
  AND NOT EXISTS (SELECT 1 FROM pg_catalog.pg_constraint cc
                  WHERE cc.conindid = i.indexrelid AND cc.contype IN ('p', 'u') AND cc.conrelid = i.indrelid)
{% if table %}
  AND c.relname = {{ bind("table") }}
{% endif %}
ORDER BY c.relname, ic.relname, i.pos

-- [triggers]
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
  AND c.relpersistence <> 't'
{% if table %}
  AND c.relname = {{ bind("table") }}
{% endif %}
ORDER BY c.relname, t.tgname

-- [referencing_keys]
SELECT n.nspname AS table_schema,
       c.relname AS table_name,
       con.conname AS constraint_name,
       a.attname AS column_name,
       con.pos::int AS position,
       ra.attname AS ref_column,
       CASE con.confdeltype WHEN 'c' THEN 'CASCADE' WHEN 'n' THEN 'SET NULL' WHEN 'd' THEN 'SET DEFAULT'
                         WHEN 'r' THEN 'RESTRICT' ELSE 'NO ACTION' END AS on_delete,
       CASE con.confupdtype WHEN 'c' THEN 'CASCADE' WHEN 'n' THEN 'SET NULL' WHEN 'd' THEN 'SET DEFAULT'
                         WHEN 'r' THEN 'RESTRICT' ELSE 'NO ACTION' END AS on_update
FROM (SELECT x.*, pg_catalog.generate_series(1, pg_catalog.array_length(x.conkey, 1)) AS pos
        FROM pg_catalog.pg_constraint x) con
JOIN pg_catalog.pg_class c ON c.oid = con.conrelid
JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace
JOIN pg_catalog.pg_class t ON t.oid = con.confrelid
JOIN pg_catalog.pg_namespace tn ON tn.oid = t.relnamespace
JOIN pg_catalog.pg_attribute a ON a.attrelid = con.conrelid AND a.attnum = con.conkey[con.pos]
JOIN pg_catalog.pg_attribute ra ON ra.attrelid = con.confrelid AND ra.attnum = con.confkey[con.pos]
WHERE con.contype = 'f'
  AND tn.nspname = {{ bind("schema") }}
  AND t.relname = {{ bind("table") }}
  AND con.conrelid <> con.confrelid
ORDER BY n.nspname, c.relname, con.conname, con.pos

-- [grants]
-- No LATERAL: aclexplode runs in a subquery.
SELECT CASE WHEN (c.acl).grantee = 0 THEN 'PUBLIC' ELSE pg_catalog.pg_get_userbyid((c.acl).grantee) END AS grantee,
       (c.acl).privilege_type AS privilege,
       (c.acl).is_grantable AS grantable
FROM (SELECT x.relname, x.relnamespace, x.relowner, pg_catalog.aclexplode(x.relacl) AS acl
        FROM pg_catalog.pg_class x) c
JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace
WHERE n.nspname = {{ bind("schema") }}
  AND c.relname = {{ bind("table") }}
  AND (c.acl).grantee <> c.relowner
ORDER BY 1, 2

-- [synonyms]
SELECT s.synname AS name,
       s.synobjschema AS target_schema,
       s.synobjname AS target_name,
       NULL AS db_link
FROM pg_catalog.pg_synonym s
JOIN pg_catalog.pg_namespace n ON n.oid = s.synnamespace
WHERE n.nspname = {{ bind("schema") }}
{% if filter %}
  AND UPPER(s.synname) LIKE UPPER({{ bind("filter") }}) ESCAPE '!'
{% endif %}
ORDER BY s.synname

-- [routines]
-- Functions and procedures (aggregates are prokind f too, proisagg filters them; routines of packages have
-- propackageid set), plus packages from gs_package (A compatibility only). Every package is listed as
-- PACKAGE and PACKAGE BODY.
SELECT r.name, r.kind, r.comment
FROM (
  SELECT p.proname AS name,
         CASE p.prokind WHEN 'p' THEN 'PROCEDURE' ELSE 'FUNCTION' END AS kind,
         MAX(pg_catalog.obj_description(p.oid, 'pg_proc')) AS comment
  FROM pg_catalog.pg_proc p
  JOIN pg_catalog.pg_namespace n ON n.oid = p.pronamespace
  WHERE n.nspname = {{ bind("schema") }}
    AND p.prokind IN ('f', 'p') AND NOT p.proisagg AND NOT p.proiswindow AND p.propackageid = 0
{% if filter %}
    AND (UPPER(p.proname) LIKE UPPER({{ bind("filter") }}) ESCAPE '!'
         OR UPPER(pg_catalog.obj_description(p.oid, 'pg_proc')) LIKE UPPER({{ bind("filter") }}) ESCAPE '!')
{% endif %}
  GROUP BY p.proname, 2
  UNION ALL
  SELECT k.pkgname, x.kind, NULL
  FROM pg_catalog.gs_package k
  JOIN pg_catalog.pg_namespace n ON n.oid = k.pkgnamespace
  CROSS JOIN (SELECT 'PACKAGE' AS kind UNION ALL SELECT 'PACKAGE BODY') x
  WHERE n.nspname = {{ bind("schema") }}
{% if filter %}
    AND UPPER(k.pkgname) LIKE UPPER({{ bind("filter") }}) ESCAPE '!'
{% endif %}
) r
ORDER BY r.name, r.kind

-- [routine_source]
-- A package is rebuilt from gs_package: pkgspecsrc / pkgbodydeclsrc start with "PACKAGE DECLARE", which is
-- dropped, and end without the final semicolon. (substring, not regexp_replace with '': in A compatibility
-- '' is NULL.)
SELECT r.line, r.text
FROM (
  SELECT (ROW_NUMBER() OVER (ORDER BY pg_catalog.pg_get_function_identity_arguments(p.oid), p.oid))::int AS line,
         pg_catalog.pg_get_functiondef(p.oid) || E'\n' AS text
  FROM pg_catalog.pg_proc p
  JOIN pg_catalog.pg_namespace n ON n.oid = p.pronamespace
  WHERE n.nspname = {{ bind("schema") }}
    AND p.proname = {{ bind("name") }}
    AND p.prokind IN ('f', 'p') AND NOT p.proisagg AND NOT p.proiswindow AND p.propackageid = 0
{% if kind %}
    AND p.prokind = CASE {{ bind("kind") }}::text WHEN 'PROCEDURE' THEN 'p' WHEN 'FUNCTION' THEN 'f' END
{% endif %}
  UNION ALL
  SELECT 1, 'CREATE OR REPLACE PACKAGE ' || pg_catalog.quote_ident(n.nspname) || '.' || pg_catalog.quote_ident(k.pkgname)
            || ' AS' || substring(k.pkgspecsrc FROM '^\s*PACKAGE\s+DECLARE(.*)$') || ';' || E'\n'
  FROM pg_catalog.gs_package k
  JOIN pg_catalog.pg_namespace n ON n.oid = k.pkgnamespace
  WHERE n.nspname = {{ bind("schema") }}
    AND k.pkgname = {{ bind("name") }}
{% if kind %}
    AND {{ bind("kind") }}::text = 'PACKAGE'
{% else %}
    AND 1 = 0
{% endif %}
  UNION ALL
  SELECT 1, 'CREATE OR REPLACE PACKAGE BODY ' || pg_catalog.quote_ident(n.nspname) || '.' || pg_catalog.quote_ident(k.pkgname)
            || ' AS' || substring(k.pkgbodydeclsrc FROM '^\s*PACKAGE\s+DECLARE(.*)$') || ';' || E'\n'
  FROM pg_catalog.gs_package k
  JOIN pg_catalog.pg_namespace n ON n.oid = k.pkgnamespace
  WHERE n.nspname = {{ bind("schema") }}
    AND k.pkgname = {{ bind("name") }}
    AND substring(k.pkgbodydeclsrc FROM '^\s*PACKAGE\s+DECLARE(.*)$') IS NOT NULL
{% if kind %}
    AND {{ bind("kind") }}::text = 'PACKAGE BODY'
{% else %}
    AND 1 = 0
{% endif %}
) r
ORDER BY r.line
