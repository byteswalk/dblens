-- SQLite metadata queries: sqlite_schema plus the pragma table-valued functions (3.16+; the bundled SQLite is current).
-- A "schema" is an attached database: main and every ATTACH (temp is hidden). Names cannot be bound in FROM, so the
-- schema is interpolated as a quoted identifier (schema_ident) and also bound where a pragma takes it as an argument.
-- Internal tables (sqlite_*) are excluded everywhere.
-- What the catalog lacks is filled in by the assembler from the CREATE text (meta::sqlite): CHECK constraints,
-- generated-column expressions, constraint names, expression-index columns, trigger timing and events.
-- The constraint_name values below are only grouping keys; the assembler replaces or clears them.

-- [schemas]
SELECT name, 0 AS is_system
FROM pragma_database_list
WHERE name <> 'temp'
{% if filter %}
  AND UPPER(name) LIKE UPPER({{ bind("filter") }}) ESCAPE '!'
{% endif %}
ORDER BY seq

-- [tables]
SELECT m.name, NULL AS comment, NULL AS partition
FROM {{ schema_ident }}.sqlite_schema m
WHERE m.type = 'table' AND m.name NOT LIKE 'sqlite!_%' ESCAPE '!'
{% if table %}
  AND m.name = {{ bind("table") }}
{% endif %}
{% if filter %}
  AND UPPER(m.name) LIKE UPPER({{ bind("filter") }}) ESCAPE '!'
{% endif %}
ORDER BY m.name

-- [views]
SELECT m.name, NULL AS comment
FROM {{ schema_ident }}.sqlite_schema m
WHERE m.type = 'view'
{% if filter %}
  AND UPPER(m.name) LIKE UPPER({{ bind("filter") }}) ESCAPE '!'
{% endif %}
ORDER BY m.name

-- [columns]
-- hidden: 0 normal, 1 hidden column of a virtual table (skipped), 2 VIRTUAL generated, 3 STORED generated.
-- auto_increment: the single-column INTEGER PRIMARY KEY of a rowid table is an alias of the rowid.
SELECT m.name AS table_name,
       c.name,
       c.cid + 1 AS ordinal,
       c.type AS type_text,
       NOT c."notnull" AS nullable,
       c.dflt_value AS default_value,
       NULL AS comment,
       (m.type = 'table' AND c.pk = 1 AND UPPER(c.type) = 'INTEGER'
        AND (SELECT COUNT(*) FROM pragma_table_xinfo(m.name, {{ bind("schema") }}) k WHERE k.pk > 0) = 1
        AND NOT EXISTS (SELECT 1 FROM pragma_table_list l
                        WHERE l.schema = {{ bind("schema") }} AND l.name = m.name AND l.wr = 1)) AS auto_increment,
       CASE c.hidden WHEN 2 THEN 'VIRTUAL' WHEN 3 THEN 'STORED' END AS generated
FROM {{ schema_ident }}.sqlite_schema m
JOIN pragma_table_xinfo(m.name, {{ bind("schema") }}) c
WHERE m.type IN ('table', 'view') AND m.name NOT LIKE 'sqlite!_%' ESCAPE '!'
  AND c.hidden <> 1
{% if table %}
  AND m.name = {{ bind("table") }}
{% endif %}
ORDER BY m.name, c.cid

-- [keys]
-- Primary key from table_xinfo.pk, UNIQUE constraints from index_list (origin 'u'), foreign keys from
-- foreign_key_list (a NULL "to" means the referenced table's primary key).
SELECT * FROM (
  SELECT m.name AS table_name, 'pk' AS constraint_name, 'P' AS constraint_type, c.name AS column_name,
         c.pk AS position, NULL AS ref_schema, NULL AS ref_table, NULL AS ref_column,
         NULL AS on_delete, NULL AS on_update
  FROM {{ schema_ident }}.sqlite_schema m
  JOIN pragma_table_xinfo(m.name, {{ bind("schema") }}) c
  WHERE m.type = 'table' AND c.pk > 0
{% if table %}
    AND m.name = {{ bind("table") }}
{% endif %}
  UNION ALL
  SELECT m.name, 'u:' || il.name, 'U', ii.name, ii.seqno + 1, NULL, NULL, NULL, NULL, NULL
  FROM {{ schema_ident }}.sqlite_schema m
  JOIN pragma_index_list(m.name, {{ bind("schema") }}) il
  JOIN pragma_index_info(il.name, {{ bind("schema") }}) ii
  WHERE m.type = 'table' AND il.origin = 'u'
{% if table %}
    AND m.name = {{ bind("table") }}
{% endif %}
  UNION ALL
  SELECT m.name, printf('fk:%04d', fk.id), 'F', fk."from", fk.seq + 1, {{ bind("schema") }}, fk."table",
         COALESCE(fk."to", (SELECT p.name FROM pragma_table_xinfo(fk."table", {{ bind("schema") }}) p
                            WHERE p.pk = fk.seq + 1)),
         fk.on_delete, fk.on_update
  FROM {{ schema_ident }}.sqlite_schema m
  JOIN pragma_foreign_key_list(m.name, {{ bind("schema") }}) fk
  WHERE m.type = 'table'
{% if table %}
    AND m.name = {{ bind("table") }}
{% endif %}
)
ORDER BY table_name, constraint_name, position

-- [checks]
-- One row per table; expression is the whole CREATE TABLE, parsed by the assembler (CHECK constraints, generated
-- expressions and constraint names all come from it).
SELECT m.name AS table_name, NULL AS constraint_name, m.sql AS expression
FROM {{ schema_ident }}.sqlite_schema m
WHERE m.type = 'table' AND m.name NOT LIKE 'sqlite!_%' ESCAPE '!'
{% if table %}
  AND m.name = {{ bind("table") }}
{% endif %}
ORDER BY m.name

-- [indexes]
-- CREATE INDEX only (origin 'c'); key columns only (key = 1). An expression column has cid -2 and no name: the
-- assembler takes it from index_sql.
SELECT m.name AS table_name,
       il.name AS index_name,
       il."unique" AS is_unique,
       NULL AS method,
       x.name AS column_expr,
       x.seqno + 1 AS position,
       x."desc" AS descending,
       x.cid AS cid,
       s.sql AS index_sql
FROM {{ schema_ident }}.sqlite_schema m
JOIN pragma_index_list(m.name, {{ bind("schema") }}) il
JOIN pragma_index_xinfo(il.name, {{ bind("schema") }}) x
JOIN {{ schema_ident }}.sqlite_schema s ON s.type = 'index' AND s.name = il.name
WHERE m.type = 'table' AND il.origin = 'c' AND x.key = 1
{% if table %}
  AND m.name = {{ bind("table") }}
{% endif %}
ORDER BY m.name, il.name, x.seqno

-- [triggers]
-- timing and events are read from the CREATE TRIGGER text by the assembler.
SELECT m.tbl_name AS table_name, m.name, NULL AS timing, '' AS events, m.sql AS definition
FROM {{ schema_ident }}.sqlite_schema m
WHERE m.type = 'trigger'
{% if table %}
  AND m.tbl_name = {{ bind("table") }}
{% endif %}
ORDER BY m.tbl_name, m.name

-- [view_definition]
SELECT m.sql AS definition
FROM {{ schema_ident }}.sqlite_schema m
WHERE m.type = 'view' AND m.name = {{ bind("name") }}

-- [table_storage]
-- No cheap row estimate (sqlite_stat1 exists only after ANALYZE); the ops page has sizes from dbstat.
SELECT NULL AS engine, NULL AS "charset", NULL AS "collation", NULL AS tablespace, NULL AS row_count
FROM {{ schema_ident }}.sqlite_schema m
WHERE m.type = 'table' AND m.name = {{ bind("table") }}
