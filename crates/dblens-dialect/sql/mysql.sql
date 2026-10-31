-- MySQL metadata queries (base template). Minimum server: MySQL 8.0.13 for full fidelity
-- (EXPRESSION in STATISTICS 8.0.13, CHECK_CONSTRAINTS 8.0.16, KEYWORDS 8.0.13).
-- Older versions degrade through the {% if version %} blocks (no functional indexes / no checks / see reserved_words).
-- All booleans come back as integer 1/0. Names are compared as bound strings against information_schema
-- (case-sensitive when lower_case_table_names=0, as on Linux; case-insensitive on 1/2).
-- MySQL has no materialized_views, synonyms, sequences and no view comments: those keys are omitted.

-- [schemas]
SELECT s.SCHEMA_NAME AS name,
       CASE WHEN LOWER(s.SCHEMA_NAME) IN ('mysql', 'sys', 'information_schema', 'performance_schema', 'metrics_schema', 'oceanbase') THEN 1 ELSE 0 END AS is_system
FROM information_schema.SCHEMATA s
ORDER BY s.SCHEMA_NAME

-- [tables]
SELECT t.TABLE_NAME AS name,
       NULLIF(t.TABLE_COMMENT, '') AS comment,
       CASE WHEN t.CREATE_OPTIONS LIKE '%partitioned%' THEN
         (SELECT CONCAT(p.PARTITION_METHOD, ' (', COALESCE(p.PARTITION_EXPRESSION, ''), ')')
            FROM information_schema.PARTITIONS p
           WHERE p.TABLE_SCHEMA = t.TABLE_SCHEMA AND p.TABLE_NAME = t.TABLE_NAME
           ORDER BY p.PARTITION_ORDINAL_POSITION LIMIT 1)
       END AS `partition`
FROM information_schema.TABLES t
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

-- [views]
SELECT v.TABLE_NAME AS name, NULL AS comment
FROM information_schema.VIEWS v
WHERE v.TABLE_SCHEMA = {{ bind("schema") }}
{% if filter %}
  AND UPPER(v.TABLE_NAME) LIKE UPPER({{ bind("filter") }}) ESCAPE '!'
{% endif %}
ORDER BY v.TABLE_NAME

-- [routines]
SELECT r.ROUTINE_NAME AS name, r.ROUTINE_TYPE AS kind, NULLIF(r.ROUTINE_COMMENT, '') AS comment
FROM information_schema.ROUTINES r
WHERE r.ROUTINE_SCHEMA = {{ bind("schema") }}
  AND r.ROUTINE_TYPE IN ('PROCEDURE', 'FUNCTION')
{% if filter %}
  AND (UPPER(r.ROUTINE_NAME) LIKE UPPER({{ bind("filter") }}) ESCAPE '!'
       OR UPPER(COALESCE(r.ROUTINE_COMMENT, '')) LIKE UPPER({{ bind("filter") }}) ESCAPE '!')
{% endif %}
ORDER BY r.ROUTINE_NAME, r.ROUTINE_TYPE

-- [columns]
-- Covers tables and views. default_value: expression defaults (EXTRA has DEFAULT_GENERATED) as raw text,
-- numeric/bit/year literals raw, all other literals SQL-quoted ('无名'), so the text is valid DDL.
SELECT c.TABLE_NAME AS table_name,
       c.COLUMN_NAME AS name,
       c.ORDINAL_POSITION AS ordinal,
       c.COLUMN_TYPE AS type_text,
       CASE WHEN c.IS_NULLABLE = 'YES' THEN 1 ELSE 0 END AS nullable,
       CASE
         WHEN c.COLUMN_DEFAULT IS NULL THEN NULL
         WHEN c.GENERATION_EXPRESSION <> '' THEN NULL
         WHEN c.EXTRA LIKE '%DEFAULT\_GENERATED%' THEN c.COLUMN_DEFAULT
         -- OceanBase 的 EXTRA 中没有 DEFAULT_GENERATED：时间列的 CURRENT_TIMESTAMP 默认值按表达式处理
         WHEN c.DATA_TYPE IN ('datetime', 'timestamp') AND UPPER(c.COLUMN_DEFAULT) LIKE 'CURRENT\_TIMESTAMP%' THEN c.COLUMN_DEFAULT
         WHEN c.DATA_TYPE IN ('tinyint', 'smallint', 'mediumint', 'int', 'bigint', 'decimal', 'float', 'double', 'year', 'bit') THEN c.COLUMN_DEFAULT
         ELSE CONCAT('''', REPLACE(REPLACE(c.COLUMN_DEFAULT, '\\', '\\\\'), '''', ''''''), '''')
       END AS default_value,
       NULLIF(c.COLUMN_COMMENT, '') AS comment,
       CASE WHEN c.EXTRA LIKE '%auto\_increment%' THEN 1 ELSE 0 END AS auto_increment,
       NULLIF(c.GENERATION_EXPRESSION, '') AS `generated`
FROM information_schema.COLUMNS c
WHERE c.TABLE_SCHEMA = {{ bind("schema") }}
{% if table %}
  AND c.TABLE_NAME = {{ bind("table") }}
{% endif %}
ORDER BY c.TABLE_NAME, c.ORDINAL_POSITION

-- [keys]
SELECT tc.TABLE_NAME AS table_name,
       tc.CONSTRAINT_NAME AS constraint_name,
       CASE tc.CONSTRAINT_TYPE WHEN 'PRIMARY KEY' THEN 'P' WHEN 'UNIQUE' THEN 'U' ELSE 'F' END AS constraint_type,
       k.COLUMN_NAME AS column_name,
       k.ORDINAL_POSITION AS position,
       k.REFERENCED_TABLE_SCHEMA AS ref_schema,
       k.REFERENCED_TABLE_NAME AS ref_table,
       k.REFERENCED_COLUMN_NAME AS ref_column,
       rc.DELETE_RULE AS on_delete,
       rc.UPDATE_RULE AS on_update
FROM information_schema.TABLE_CONSTRAINTS tc
JOIN information_schema.KEY_COLUMN_USAGE k
  ON k.CONSTRAINT_SCHEMA = tc.CONSTRAINT_SCHEMA
 AND k.TABLE_NAME = tc.TABLE_NAME
 AND k.CONSTRAINT_NAME = tc.CONSTRAINT_NAME
LEFT JOIN information_schema.REFERENTIAL_CONSTRAINTS rc
  ON tc.CONSTRAINT_TYPE = 'FOREIGN KEY'
 AND rc.CONSTRAINT_SCHEMA = tc.CONSTRAINT_SCHEMA
 AND rc.TABLE_NAME = tc.TABLE_NAME
 AND rc.CONSTRAINT_NAME = tc.CONSTRAINT_NAME
WHERE tc.TABLE_SCHEMA = {{ bind("schema") }}
  AND tc.CONSTRAINT_TYPE IN ('PRIMARY KEY', 'UNIQUE', 'FOREIGN KEY')
{% if version >= 80013 %}
  AND (tc.CONSTRAINT_TYPE <> 'UNIQUE'
       OR NOT EXISTS (SELECT 1 FROM information_schema.STATISTICS s2
                       WHERE s2.TABLE_SCHEMA = tc.TABLE_SCHEMA AND s2.TABLE_NAME = tc.TABLE_NAME
                         AND s2.INDEX_NAME = tc.CONSTRAINT_NAME AND s2.EXPRESSION IS NOT NULL))
{% endif %}
{% if table %}
  AND tc.TABLE_NAME = {{ bind("table") }}
{% endif %}
ORDER BY tc.TABLE_NAME, tc.CONSTRAINT_NAME, k.ORDINAL_POSITION

-- [checks]
{% if version >= 80016 %}
SELECT tc.TABLE_NAME AS table_name,
       tc.CONSTRAINT_NAME AS constraint_name,
       cc.CHECK_CLAUSE AS expression
FROM information_schema.TABLE_CONSTRAINTS tc
JOIN information_schema.CHECK_CONSTRAINTS cc
  ON cc.CONSTRAINT_SCHEMA = tc.CONSTRAINT_SCHEMA
 AND cc.CONSTRAINT_NAME = tc.CONSTRAINT_NAME
WHERE tc.TABLE_SCHEMA = {{ bind("schema") }}
  AND tc.CONSTRAINT_TYPE = 'CHECK'
{% if table %}
  AND tc.TABLE_NAME = {{ bind("table") }}
{% endif %}
ORDER BY tc.TABLE_NAME, tc.CONSTRAINT_NAME
{% else %}
SELECT NULL AS table_name, NULL AS constraint_name, NULL AS expression FROM DUAL WHERE 1 = 0
{% endif %}

-- [indexes]
-- Non-unique indexes only: every unique index is already a UNIQUE constraint in `keys`.
-- Exception (8.0.13+): a unique index containing a functional part has no KEY_COLUMN_USAGE rows,
-- so it would vanish from `keys`; it is kept here instead.
SELECT s.TABLE_NAME AS table_name,
       s.INDEX_NAME AS index_name,
       CASE WHEN s.NON_UNIQUE = 0 THEN 1 ELSE 0 END AS is_unique,
       s.INDEX_TYPE AS method,
{% if version >= 80013 %}
       COALESCE(s.COLUMN_NAME, s.EXPRESSION) AS column_expr,
{% else %}
       s.COLUMN_NAME AS column_expr,
{% endif %}
       s.SEQ_IN_INDEX AS position,
       CASE WHEN s.COLLATION = 'D' THEN 1 ELSE 0 END AS descending
FROM information_schema.STATISTICS s
WHERE s.TABLE_SCHEMA = {{ bind("schema") }}
{% if version >= 80013 %}
  AND (s.NON_UNIQUE = 1
       OR EXISTS (SELECT 1 FROM information_schema.STATISTICS s2
                   WHERE s2.TABLE_SCHEMA = s.TABLE_SCHEMA AND s2.TABLE_NAME = s.TABLE_NAME
                     AND s2.INDEX_NAME = s.INDEX_NAME AND s2.EXPRESSION IS NOT NULL))
{% else %}
  AND s.NON_UNIQUE = 1
{% endif %}
{% if table %}
  AND s.TABLE_NAME = {{ bind("table") }}
{% endif %}
ORDER BY s.TABLE_NAME, s.INDEX_NAME, s.SEQ_IN_INDEX

-- [triggers]
-- definition: a CREATE TRIGGER statement rebuilt from TRIGGERS (FOLLOWS/PRECEDES order and DEFINER are not included).
-- MySQL allows exactly one event per trigger, so events is that single event.
SELECT t.EVENT_OBJECT_TABLE AS table_name,
       t.TRIGGER_NAME AS name,
       t.ACTION_TIMING AS timing,
       t.EVENT_MANIPULATION AS events,
       CONCAT('CREATE TRIGGER `', REPLACE(t.TRIGGER_NAME, '`', '``'), '` ', t.ACTION_TIMING, ' ', t.EVENT_MANIPULATION,
              ' ON `', REPLACE(t.EVENT_OBJECT_TABLE, '`', '``'), '` FOR EACH ROW ', t.ACTION_STATEMENT) AS definition
FROM information_schema.TRIGGERS t
WHERE t.EVENT_OBJECT_SCHEMA = {{ bind("schema") }}
{% if table %}
  AND t.EVENT_OBJECT_TABLE = {{ bind("table") }}
{% endif %}
ORDER BY t.EVENT_OBJECT_TABLE, t.TRIGGER_NAME

-- [view_definition]
SELECT v.VIEW_DEFINITION AS definition
FROM information_schema.VIEWS v
WHERE v.TABLE_SCHEMA = {{ bind("schema") }}
  AND v.TABLE_NAME = {{ bind("name") }}

-- [routine_source]
-- SHOW CREATE cannot take bound params, so the CREATE statement is rebuilt from ROUTINES + PARAMETERS
-- (params, RETURNS, DETERMINISTIC, SQL SECURITY, COMMENT, body). One row, line = 1. Character-set clauses are not reproduced.
SELECT 1 AS line,
       CONCAT('CREATE ', r.ROUTINE_TYPE, ' `', REPLACE(r.ROUTINE_NAME, '`', '``'), '`(',
              COALESCE((SELECT GROUP_CONCAT(
                                 CONCAT(CASE WHEN r.ROUTINE_TYPE = 'PROCEDURE' THEN CONCAT(COALESCE(p.PARAMETER_MODE, 'IN'), ' ') ELSE '' END,
                                        '`', REPLACE(p.PARAMETER_NAME, '`', '``'), '` ', p.DTD_IDENTIFIER)
                                 ORDER BY p.ORDINAL_POSITION SEPARATOR ', ')
                          FROM information_schema.PARAMETERS p
                         WHERE p.SPECIFIC_SCHEMA = r.ROUTINE_SCHEMA AND p.SPECIFIC_NAME = r.SPECIFIC_NAME
                           AND p.ROUTINE_TYPE = r.ROUTINE_TYPE AND p.ORDINAL_POSITION > 0), ''),
              ')',
              CASE WHEN r.ROUTINE_TYPE = 'FUNCTION' THEN CONCAT(' RETURNS ', r.DTD_IDENTIFIER) ELSE '' END,
              CHAR(10), '    ', r.SQL_DATA_ACCESS,
              CASE WHEN r.IS_DETERMINISTIC = 'YES' THEN ' DETERMINISTIC' ELSE '' END,
              ' SQL SECURITY ', r.SECURITY_TYPE,
              CASE WHEN r.ROUTINE_COMMENT <> '' THEN CONCAT(' COMMENT ''', REPLACE(REPLACE(r.ROUTINE_COMMENT, '\\', '\\\\'), '''', ''''''), '''') ELSE '' END,
              CHAR(10), COALESCE(r.ROUTINE_DEFINITION, '')) AS text
FROM information_schema.ROUTINES r
WHERE r.ROUTINE_SCHEMA = {{ bind("schema") }}
  AND r.ROUTINE_NAME = {{ bind("name") }}
  AND r.ROUTINE_TYPE = {{ bind("kind") }}

-- [reserved_words]
SELECT k.WORD AS word
FROM information_schema.KEYWORDS k
WHERE k.RESERVED = 1
ORDER BY k.WORD

-- [variant_probe]
SHOW VARIABLES LIKE 'polardb_version'

-- [referencing_keys]
-- Table designer rebuild: foreign keys of OTHER tables that reference this table (dropped and recreated around the swap).
SELECT k.TABLE_SCHEMA AS table_schema,
       k.TABLE_NAME AS table_name,
       k.CONSTRAINT_NAME AS constraint_name,
       k.COLUMN_NAME AS column_name,
       k.ORDINAL_POSITION AS position,
       k.REFERENCED_COLUMN_NAME AS ref_column,
       rc.DELETE_RULE AS on_delete,
       rc.UPDATE_RULE AS on_update
FROM information_schema.KEY_COLUMN_USAGE k
JOIN information_schema.REFERENTIAL_CONSTRAINTS rc
  ON rc.CONSTRAINT_SCHEMA = k.CONSTRAINT_SCHEMA
 AND rc.TABLE_NAME = k.TABLE_NAME
 AND rc.CONSTRAINT_NAME = k.CONSTRAINT_NAME
WHERE k.REFERENCED_TABLE_SCHEMA = {{ bind("schema") }}
  AND k.REFERENCED_TABLE_NAME = {{ bind("table") }}
  AND NOT (k.TABLE_SCHEMA = k.REFERENCED_TABLE_SCHEMA AND k.TABLE_NAME = k.REFERENCED_TABLE_NAME)
ORDER BY k.TABLE_SCHEMA, k.TABLE_NAME, k.CONSTRAINT_NAME, k.ORDINAL_POSITION

-- [table_storage]
-- Table designer: engine / charset / collation kept by a rebuild, and the row estimate shown in the designer.
SELECT t.ENGINE AS engine,
       (SELECT MAX(c.CHARACTER_SET_NAME) FROM information_schema.COLLATION_CHARACTER_SET_APPLICABILITY c
         WHERE c.COLLATION_NAME = t.TABLE_COLLATION) AS charset,
       t.TABLE_COLLATION AS collation,
       NULL AS tablespace,
       t.TABLE_ROWS AS row_count
FROM information_schema.TABLES t
WHERE t.TABLE_SCHEMA = {{ bind("schema") }}
  AND t.TABLE_NAME = {{ bind("table") }}
