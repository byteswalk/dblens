-- TiDB overrides on top of mysql.sql (only keys that differ). Tested on v8.5.8; VERSION() reports 8.0.11.

-- [checks]
-- CHECK constraints are kept only while tidb_enable_check_constraint is ON (7.2+; with it OFF they are
-- parsed and dropped). They are not in TABLE_CONSTRAINTS; TIDB_CHECK_CONSTRAINTS carries the table name.
SELECT c.TABLE_NAME AS table_name,
       c.CONSTRAINT_NAME AS constraint_name,
       c.CHECK_CLAUSE AS expression
FROM information_schema.TIDB_CHECK_CONSTRAINTS c
WHERE c.CONSTRAINT_SCHEMA = {{ bind("schema") }}
{% if table %}
  AND c.TABLE_NAME = {{ bind("table") }}
{% endif %}
ORDER BY c.TABLE_NAME, c.CONSTRAINT_NAME

-- [tables]
-- Partition from a join: a correlated subquery over information_schema returns NULL for every row once
-- more than one table matches (TiDB 8.5).
SELECT t.TABLE_NAME AS name,
       NULLIF(t.TABLE_COMMENT, '') AS comment,
       CASE WHEN p.PARTITION_METHOD IS NOT NULL
            THEN CONCAT(p.PARTITION_METHOD, ' (', COALESCE(p.PARTITION_EXPRESSION, ''), ')')
       END AS `partition`
FROM information_schema.TABLES t
LEFT JOIN information_schema.PARTITIONS p
  ON p.TABLE_SCHEMA = t.TABLE_SCHEMA AND p.TABLE_NAME = t.TABLE_NAME AND p.PARTITION_ORDINAL_POSITION = 1
WHERE t.TABLE_SCHEMA = {{ bind("schema") }}
  AND t.TABLE_TYPE = 'BASE TABLE'
{% if table %}
  AND t.TABLE_NAME = {{ bind("table") }}
{% endif %}
{% if filter %}
  AND (UPPER(t.TABLE_NAME) LIKE UPPER({{ bind("filter") }}) ESCAPE '!'
       OR UPPER(COALESCE(t.TABLE_COMMENT, '')) LIKE UPPER({{ bind("filter") }}) ESCAPE '!')
{% endif %}
ORDER BY t.TABLE_NAME

-- [sequences]
-- TiDB supports CREATE SEQUENCE (MySQL has no sequences); CACHE is 1/0 and the cache size is CACHE_VALUE.
SELECT s.SEQUENCE_NAME AS name,
       s.START AS start_value,
       s.INCREMENT AS increment,
       s.MIN_VALUE AS min_value,
       s.MAX_VALUE AS max_value,
       s.CYCLE AS cycle,
       CASE WHEN s.CACHE = 1 THEN s.CACHE_VALUE END AS cache
FROM information_schema.SEQUENCES s
WHERE s.SEQUENCE_SCHEMA = {{ bind("schema") }}
{% if filter %}
  AND UPPER(s.SEQUENCE_NAME) LIKE UPPER({{ bind("filter") }}) ESCAPE '!'
{% endif %}
ORDER BY s.SEQUENCE_NAME

-- [columns]
-- As mysql.sql, plus AUTO_RANDOM: it is not in EXTRA; the table's TIDB_ROW_ID_SHARDING_INFO is
-- PK_AUTO_RANDOM_BITS=n and the column is its (single-column, clustered) primary key.
SELECT c.TABLE_NAME AS table_name,
       c.COLUMN_NAME AS name,
       c.ORDINAL_POSITION AS ordinal,
       c.COLUMN_TYPE AS type_text,
       CASE WHEN c.IS_NULLABLE = 'YES' THEN 1 ELSE 0 END AS nullable,
       CASE
         WHEN c.COLUMN_DEFAULT IS NULL THEN NULL
         WHEN c.GENERATION_EXPRESSION <> '' THEN NULL
         WHEN c.EXTRA LIKE '%DEFAULT\_GENERATED%' THEN c.COLUMN_DEFAULT
         WHEN c.DATA_TYPE IN ('tinyint', 'smallint', 'mediumint', 'int', 'bigint', 'decimal', 'float', 'double', 'year', 'bit') THEN c.COLUMN_DEFAULT
         ELSE CONCAT('''', REPLACE(REPLACE(c.COLUMN_DEFAULT, '\\', '\\\\'), '''', ''''''), '''')
       END AS default_value,
       NULLIF(c.COLUMN_COMMENT, '') AS comment,
       CASE WHEN c.EXTRA LIKE '%auto\_increment%'
              OR (c.COLUMN_KEY = 'PRI' AND t.TIDB_ROW_ID_SHARDING_INFO LIKE 'PK!_AUTO!_RANDOM!_BITS%' ESCAPE '!')
            THEN 1 ELSE 0 END AS auto_increment,
       NULLIF(c.GENERATION_EXPRESSION, '') AS `generated`
FROM information_schema.COLUMNS c
JOIN information_schema.TABLES t ON t.TABLE_SCHEMA = c.TABLE_SCHEMA AND t.TABLE_NAME = c.TABLE_NAME
WHERE c.TABLE_SCHEMA = {{ bind("schema") }}
{% if table %}
  AND c.TABLE_NAME = {{ bind("table") }}
{% endif %}
ORDER BY c.TABLE_NAME, c.ORDINAL_POSITION

-- [table_storage]
-- As mysql.sql, plus whether the primary key is clustered (TIDB_PK_TYPE): a clustered primary key cannot
-- be dropped or changed in place, the table designer rebuilds the table instead.
SELECT t.ENGINE AS engine,
       (SELECT MAX(c.CHARACTER_SET_NAME) FROM information_schema.COLLATION_CHARACTER_SET_APPLICABILITY c
         WHERE c.COLLATION_NAME = t.TABLE_COLLATION) AS charset,
       t.TABLE_COLLATION AS collation,
       NULL AS tablespace,
       t.TABLE_ROWS AS row_count,
       CASE WHEN t.TIDB_PK_TYPE = 'CLUSTERED' THEN 1 ELSE 0 END AS clustered
FROM information_schema.TABLES t
WHERE t.TABLE_SCHEMA = {{ bind("schema") }}
  AND t.TABLE_NAME = {{ bind("table") }}
