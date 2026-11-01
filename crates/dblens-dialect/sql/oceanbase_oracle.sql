-- OceanBase (Oracle mode) overrides on top of oracle.sql (only keys that differ). Tested on 4.4.2.2.
-- The Oracle templates run as they are at the 11.2 baseline, because the reported version is OceanBase's own.
-- Identity columns: IDENTITY_COLUMN is NULL and ALL_TAB_IDENTITY_COLS does not exist; the default reads
-- SEQUENCE.NEXTVAL instead.

-- [schemas]
-- ORACLE_MAINTAINED does not exist; these are the built-in accounts of an Oracle-mode tenant.
SELECT u.username AS name,
       CASE WHEN u.username IN ('SYS', 'LBACSYS', 'ORAAUDITOR', 'OCEANBASE') THEN 'Y' ELSE 'N' END AS is_system
FROM all_users u
ORDER BY u.username

-- [columns]
SELECT c.table_name AS table_name,
       c.column_name AS name,
       COALESCE(c.column_id, 100000 + c.internal_column_id) AS ordinal,
       CASE
         WHEN c.data_type_owner IS NOT NULL
           THEN CASE WHEN c.data_type_owner IN (c.owner, 'SYS', 'PUBLIC') THEN '' ELSE c.data_type_owner || '.' END || c.data_type
         WHEN c.data_type = 'NUMBER'
           THEN CASE WHEN c.data_precision IS NULL AND c.data_scale IS NULL THEN 'NUMBER'
                     WHEN c.data_precision IS NULL THEN 'NUMBER(*,' || c.data_scale || ')'
                     WHEN c.data_scale IS NULL OR c.data_scale = 0 THEN 'NUMBER(' || c.data_precision || ')'
                     ELSE 'NUMBER(' || c.data_precision || ',' || c.data_scale || ')' END
         WHEN c.data_type = 'FLOAT' THEN 'FLOAT(' || NVL(c.data_precision, 126) || ')'
         WHEN c.data_type IN ('VARCHAR2', 'CHAR')
           THEN c.data_type || '(' || CASE WHEN c.char_used = 'C' THEN c.char_length || ' CHAR' ELSE c.data_length || ' BYTE' END || ')'
         WHEN c.data_type IN ('NVARCHAR2', 'NCHAR') THEN c.data_type || '(' || c.char_length || ')'
         WHEN c.data_type = 'RAW' THEN 'RAW(' || c.data_length || ')'
         WHEN c.data_type = 'UROWID' AND c.data_length <> 4000 THEN 'UROWID(' || c.data_length || ')'
         ELSE c.data_type
       END AS type_text,
       c.nullable AS nullable,
       CASE WHEN TO_CHAR(d.data_default) = 'SEQUENCE.NEXTVAL' THEN NULL ELSE d.data_default END AS default_value,
       cm.comments AS "comment",
       CASE WHEN TO_CHAR(c.data_default) = 'SEQUENCE.NEXTVAL' THEN 'Y' ELSE 'N' END AS auto_increment,
       g.data_default AS generated
FROM all_tab_cols c
LEFT JOIN all_col_comments cm ON cm.owner = c.owner AND cm.table_name = c.table_name AND cm.column_name = c.column_name
LEFT JOIN all_tab_cols d ON d.owner = c.owner AND d.table_name = c.table_name AND d.column_name = c.column_name
     AND d.virtual_column = 'NO' AND d.default_length IS NOT NULL
LEFT JOIN all_tab_cols g ON g.owner = c.owner AND g.table_name = c.table_name AND g.column_name = c.column_name
     AND g.virtual_column = 'YES'
WHERE c.owner = {{ bind("schema") }}
  AND c.table_name NOT LIKE 'BIN$%'
  AND c.hidden_column = 'NO'
{% if table %}
  AND c.table_name = {{ bind("table") }}
{% else %}
  AND c.table_name NOT IN (SELECT x.table_name FROM all_all_tables x
                           WHERE x.owner = {{ bind("schema") }}
                             AND (x.nested = 'YES' OR x.iot_type IN ('IOT_OVERFLOW', 'IOT_MAPPING') OR x.secondary = 'Y' OR x.dropped = 'YES'))
{% endif %}
ORDER BY c.table_name, COALESCE(c.column_id, 100000 + c.internal_column_id)

-- [trigger_ddl]
-- DESCRIPTION is NULL here and TRIGGER_BODY already holds the whole definition, from the word TRIGGER up to the
-- final END, without the closing semicolon.
SELECT t.trigger_name AS name,
       'CREATE OR REPLACE ' AS head,
       t.trigger_body || ';' AS body
FROM all_triggers t
WHERE t.table_owner = {{ bind("schema") }}
  AND t.table_name = {{ bind("table") }}
  AND t.base_object_type = 'TABLE'
ORDER BY t.trigger_name
