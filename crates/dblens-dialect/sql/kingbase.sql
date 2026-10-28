-- KingbaseES overrides on top of postgres.sql (only keys that differ). Tested on V9R2C16 in Oracle and PG mode.

-- [variant_probe]
-- Compatibility mode (oracle / pg / mysql / sqlserver) and the PostgreSQL version the build is based on.
SELECT current_setting('database_mode') AS mode,
       current_setting('server_version_num') AS version_num

-- [schemas]
-- Built-in schemas besides pg_* and information_schema (sys, sys_*, dbms_*, ...); Oracle mode adds sys and wmsys.
SELECT n.nspname AS name,
       (n.nspname LIKE 'pg!_%' ESCAPE '!' OR n.nspname LIKE 'sys!_%' ESCAPE '!' OR n.nspname LIKE 'dbms!_%' ESCAPE '!'
        OR n.nspname LIKE 'kdb!_%' ESCAPE '!'
        OR n.nspname IN ('information_schema', 'sys', 'anon', 'perf', 'src_restrict', 'sysaudit', 'sysmac',
                         'wmsys', 'xlog_record_read')) AS is_system
FROM pg_catalog.pg_namespace n
ORDER BY n.nspname

-- [synonyms]
-- Oracle-style synonyms (both compatibility modes have them); sys_synonym is a view over the catalog.
SELECT s.synname AS name,
       s.refobjnspname AS target_schema,
       s.refobjname AS target_name,
       NULL AS db_link
FROM pg_catalog.sys_synonym s
JOIN pg_catalog.pg_namespace n ON n.oid = s.synnamespace
WHERE n.nspname = {{ bind("schema") }}
{% if filter %}
  AND UPPER(s.synname) LIKE UPPER({{ bind("filter") }}) ESCAPE '!'
{% endif %}
ORDER BY s.synname

-- [routines]
-- Functions and procedures as in postgres.sql, plus packages (pg_catalog._pkg; their routines are not in
-- pg_proc). Every package is listed as PACKAGE and PACKAGE BODY.
SELECT r.name, r.kind, r.comment
FROM (
  SELECT p.proname AS name,
         CASE p.prokind WHEN 'p' THEN 'PROCEDURE' ELSE 'FUNCTION' END AS kind,
         MAX(pg_catalog.obj_description(p.oid, 'pg_proc')) AS comment
  FROM pg_catalog.pg_proc p
  JOIN pg_catalog.pg_namespace n ON n.oid = p.pronamespace
  WHERE n.nspname = {{ bind("schema") }}
    AND p.prokind IN ('f', 'p')
{% if filter %}
    AND (UPPER(p.proname) LIKE UPPER({{ bind("filter") }}) ESCAPE '!'
         OR UPPER(pg_catalog.obj_description(p.oid, 'pg_proc')) LIKE UPPER({{ bind("filter") }}) ESCAPE '!')
{% endif %}
  GROUP BY p.proname, 2
  UNION ALL
  SELECT k.pkgname, x.kind, NULL
  FROM pg_catalog._pkg k
  JOIN pg_catalog.pg_namespace n ON n.oid = k.pkgnamespace
  CROSS JOIN (SELECT 'PACKAGE' AS kind UNION ALL SELECT 'PACKAGE BODY') x
  WHERE n.nspname = {{ bind("schema") }}
{% if filter %}
    AND UPPER(k.pkgname) LIKE UPPER({{ bind("filter") }}) ESCAPE '!'
{% endif %}
) r
ORDER BY r.name, r.kind

-- [routine_source]
-- As postgres.sql for functions and procedures; a package is rebuilt from the stored spec / body text
-- (pkgspecsrc / pkgbodysrc hold what follows AS, without the final semicolon).
SELECT r.line, r.text
FROM (
  SELECT (ROW_NUMBER() OVER (ORDER BY pg_catalog.pg_get_function_identity_arguments(p.oid), p.oid))::int AS line,
         pg_catalog.pg_get_functiondef(p.oid) || E'\n' AS text
  FROM pg_catalog.pg_proc p
  JOIN pg_catalog.pg_namespace n ON n.oid = p.pronamespace
  WHERE n.nspname = {{ bind("schema") }}
    AND p.proname = {{ bind("name") }}
    AND p.prokind IN ('f', 'p')
{% if kind %}
    AND p.prokind = CASE {{ bind("kind") }}::text WHEN 'PROCEDURE' THEN 'p' WHEN 'FUNCTION' THEN 'f' END
{% endif %}
  UNION ALL
  SELECT 1, 'CREATE OR REPLACE PACKAGE ' || pg_catalog.quote_ident(n.nspname) || '.' || pg_catalog.quote_ident(k.pkgname)
            || ' AS' || k.pkgspecsrc || ';' || E'\n'
  FROM pg_catalog._pkg k
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
            || ' AS' || k.pkgbodysrc || ';' || E'\n'
  FROM pg_catalog._pkg k
  JOIN pg_catalog.pg_namespace n ON n.oid = k.pkgnamespace
  WHERE n.nspname = {{ bind("schema") }}
    AND k.pkgname = {{ bind("name") }}
    AND k.pkgbodysrc IS NOT NULL
{% if kind %}
    AND {{ bind("kind") }}::text = 'PACKAGE BODY'
{% else %}
    AND 1 = 0
{% endif %}
) r
ORDER BY r.line
