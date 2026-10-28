-- OceanBase (MySQL mode) overrides on top of mysql.sql (only keys that differ). Tested on 4.4.2.1;
-- VERSION() reports 5.7.25, but CHECK constraints are supported and listed like MySQL 8.0.16+.
-- reserved_words is empty (removed): there is no information_schema.KEYWORDS; ident.rs has the built-in list.

-- [checks]
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

-- [indexes]
-- Non-unique indexes only (as in mysql.sql). A FULLTEXT index lists only hidden columns
-- (__doc_id_*, __word_segment_*), so with those left out it does not appear. Index columns are always
-- stored ascending (DESC is accepted and ignored).
SELECT s.TABLE_NAME AS table_name,
       s.INDEX_NAME AS index_name,
       0 AS is_unique,
       s.INDEX_TYPE AS method,
       s.COLUMN_NAME AS column_expr,
       s.SEQ_IN_INDEX AS position,
       0 AS descending
FROM information_schema.STATISTICS s
WHERE s.TABLE_SCHEMA = {{ bind("schema") }}
  AND s.NON_UNIQUE = 1
  AND s.COLUMN_NAME NOT LIKE '!_!_%' ESCAPE '!'
{% if table %}
  AND s.TABLE_NAME = {{ bind("table") }}
{% endif %}
ORDER BY s.TABLE_NAME, s.INDEX_NAME, s.SEQ_IN_INDEX

-- [reserved_words]

-- [tables]
-- Partition from a join: CREATE_OPTIONS is NULL even for partitioned tables.
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

-- [triggers]
-- ACTION_STATEMENT holds the whole statement after CREATE ("TRIGGER name BEFORE INSERT ON t FOR EACH ROW ..."),
-- not just the body.
SELECT t.EVENT_OBJECT_TABLE AS table_name,
       t.TRIGGER_NAME AS name,
       t.ACTION_TIMING AS timing,
       t.EVENT_MANIPULATION AS events,
       CONCAT('CREATE ', t.ACTION_STATEMENT) AS definition
FROM information_schema.TRIGGERS t
WHERE t.EVENT_OBJECT_SCHEMA = {{ bind("schema") }}
{% if table %}
  AND t.EVENT_OBJECT_TABLE = {{ bind("table") }}
{% endif %}
ORDER BY t.EVENT_OBJECT_TABLE, t.TRIGGER_NAME
