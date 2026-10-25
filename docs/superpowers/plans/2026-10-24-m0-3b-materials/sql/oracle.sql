-- Oracle metadata queries (ALL_* views only; no DBA_*, no V$ except `reserved_words`).
-- Baseline: Oracle 11g R2 (11.2). Guards: >=120100 (12.1) identity/USER_GENERATED/ORACLE_MAINTAINED,
-- >=120200 (12.2) the *_VC columns. Only 23ai (232603) was available for testing.
--
-- LONG columns (DATA_DEFAULT, TEXT, TRIGGER_BODY, SEARCH_CONDITION, QUERY, COLUMN_EXPRESSION) are selected AS-IS:
-- the Oracle driver returns a LONG column as Value::String (OracleType::Long -> v.get::<String>()), without
-- truncation (the *_VC twins are silently cut at 4000 chars). LONG cannot be used in functions, CASE, COALESCE,
-- DISTINCT/GROUP BY or mixed with a VARCHAR2 in a UNION, so every query below only SELECTs a LONG column directly,
-- outer-joins it, or UNION ALLs it with another LONG. Consequently the assembler MUST trim trailing whitespace
-- (Oracle stores e.g. `'x' ` / trailing newline) from default_value, generated, expression and definition.

-- [schemas]
SELECT u.username AS name,
{% if version >= 120100 %}
       u.oracle_maintained AS is_system
{% else %}
       CASE WHEN u.username IN ('SYS','SYSTEM','OUTLN','DBSNMP','APPQOSSYS','CTXSYS','MDSYS','MDDATA','ORDSYS','ORDDATA','ORDPLUGINS','SI_INFORMTN_SCHEMA','XDB','WMSYS','EXFSYS','OLAPSYS','DIP','ANONYMOUS','SYSMAN','LBACSYS','DVSYS','DVF','ORACLE_OCM','XS$NULL','MGMT_VIEW','OWBSYS','OWBSYS_AUDIT','SPATIAL_CSW_ADMIN_USR','SPATIAL_WFS_ADMIN_USR','APEX_PUBLIC_USER','FLOWS_FILES')
              OR u.username LIKE 'APEX!_%' ESCAPE '!' OR u.username LIKE 'FLOWS!_%' ESCAPE '!'
            THEN 'Y' ELSE 'N' END AS is_system
{% endif %}
FROM all_users u
ORDER BY u.username

-- [tables]
SELECT t.table_name AS name,
       c.comments AS "comment",
       CASE WHEN pt.table_name IS NULL THEN NULL
            ELSE pt.partitioning_type
                 || CASE WHEN pt.partitioning_key_count > 0 THEN ' (' || (SELECT LISTAGG(k.column_name, ', ') WITHIN GROUP (ORDER BY k.column_position) FROM all_part_key_columns k WHERE k.owner = pt.owner AND k.name = pt.table_name AND k.object_type = 'TABLE') || ')' END
                 || CASE WHEN pt.subpartitioning_type <> 'NONE' THEN ' SUBPARTITION BY ' || pt.subpartitioning_type || CASE WHEN pt.subpartitioning_key_count > 0 THEN ' (' || (SELECT LISTAGG(k.column_name, ', ') WITHIN GROUP (ORDER BY k.column_position) FROM all_subpart_key_columns k WHERE k.owner = pt.owner AND k.name = pt.table_name AND k.object_type = 'TABLE') || ')' END END
       END AS partition
FROM all_all_tables t
LEFT JOIN all_tab_comments c ON c.owner = t.owner AND c.table_name = t.table_name AND c.table_type = 'TABLE'
LEFT JOIN all_part_tables pt ON pt.owner = t.owner AND pt.table_name = t.table_name
WHERE t.owner = {{ bind("schema") }}
  AND t.table_name NOT LIKE 'BIN$%'
  AND NVL(t.dropped, 'NO') = 'NO'
  AND NVL(t.nested, 'NO') = 'NO'
  AND NVL(t.iot_type, 'X') NOT IN ('IOT_OVERFLOW', 'IOT_MAPPING')
  AND NVL(t.secondary, 'N') = 'N'
  AND t.table_name NOT IN (SELECT m.mview_name FROM all_mviews m WHERE m.owner = {{ bind("schema") }})
{% if table %}
  AND t.table_name = {{ bind("table") }}
{% endif %}
{% if filter %}
  AND (UPPER(t.table_name) LIKE UPPER({{ bind("filter") }}) ESCAPE '!' OR UPPER(c.comments) LIKE UPPER({{ bind("filter") }}) ESCAPE '!')
{% endif %}
ORDER BY t.table_name

-- [views]
SELECT v.view_name AS name,
       c.comments AS "comment"
FROM all_views v
LEFT JOIN all_tab_comments c ON c.owner = v.owner AND c.table_name = v.view_name AND c.table_type = 'VIEW'
WHERE v.owner = {{ bind("schema") }}
  AND v.view_name NOT LIKE 'BIN$%'
{% if filter %}
  AND (UPPER(v.view_name) LIKE UPPER({{ bind("filter") }}) ESCAPE '!' OR UPPER(c.comments) LIKE UPPER({{ bind("filter") }}) ESCAPE '!')
{% endif %}
ORDER BY v.view_name

-- [materialized_views]
SELECT m.mview_name AS name,
       c.comments AS "comment"
FROM all_mviews m
LEFT JOIN all_mview_comments c ON c.owner = m.owner AND c.mview_name = m.mview_name
WHERE m.owner = {{ bind("schema") }}
{% if filter %}
  AND (UPPER(m.mview_name) LIKE UPPER({{ bind("filter") }}) ESCAPE '!' OR UPPER(c.comments) LIKE UPPER({{ bind("filter") }}) ESCAPE '!')
{% endif %}
ORDER BY m.mview_name

-- [sequences]
SELECT s.sequence_name AS name,
       CAST(NULL AS NUMBER) AS start_value,
       s.increment_by AS "increment",
       s.min_value AS min_value,
       s.max_value AS max_value,
       s.cycle_flag AS cycle,
       s.cache_size AS cache
FROM all_sequences s
WHERE s.sequence_owner = {{ bind("schema") }}
  AND s.sequence_name NOT LIKE 'ISEQ$$!_%' ESCAPE '!'
{% if filter %}
  AND UPPER(s.sequence_name) LIKE UPPER({{ bind("filter") }}) ESCAPE '!'
{% endif %}
ORDER BY s.sequence_name

-- [routines]
SELECT o.object_name AS name,
       o.object_type AS kind,
       CAST(NULL AS VARCHAR2(1)) AS "comment"
FROM all_objects o
WHERE o.owner = {{ bind("schema") }}
  AND o.object_type IN ('PROCEDURE', 'FUNCTION', 'PACKAGE', 'PACKAGE BODY')
  AND o.object_name NOT LIKE 'BIN$%'
{% if filter %}
  AND UPPER(o.object_name) LIKE UPPER({{ bind("filter") }}) ESCAPE '!'
{% endif %}
ORDER BY o.object_name, o.object_type

-- [synonyms]
SELECT s.synonym_name AS name,
       s.table_owner AS target_schema,
       s.table_name AS target_name,
       s.db_link AS db_link
FROM all_synonyms s
WHERE s.owner = {{ bind("schema") }}
{% if filter %}
  AND UPPER(s.synonym_name) LIKE UPPER({{ bind("filter") }}) ESCAPE '!'
{% endif %}
ORDER BY s.synonym_name

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
       d.data_default AS default_value,
       cm.comments AS "comment",
{% if version >= 120100 %}
       CASE WHEN c.identity_column = 'YES' THEN 'Y' ELSE 'N' END AS auto_increment,
{% else %}
       'N' AS auto_increment,
{% endif %}
       g.data_default AS generated
FROM all_tab_cols c
LEFT JOIN all_col_comments cm ON cm.owner = c.owner AND cm.table_name = c.table_name AND cm.column_name = c.column_name
LEFT JOIN all_tab_cols d ON d.owner = c.owner AND d.table_name = c.table_name AND d.column_name = c.column_name
     AND d.virtual_column = 'NO' AND d.default_length IS NOT NULL
{% if version >= 120100 %}
     AND d.identity_column = 'NO'
{% endif %}
LEFT JOIN all_tab_cols g ON g.owner = c.owner AND g.table_name = c.table_name AND g.column_name = c.column_name
     AND g.virtual_column = 'YES'
WHERE c.owner = {{ bind("schema") }}
  AND c.table_name NOT LIKE 'BIN$%'
{% if version >= 120100 %}
  AND c.user_generated = 'YES'
{% else %}
  AND c.hidden_column = 'NO'
{% endif %}
{% if table %}
  AND c.table_name = {{ bind("table") }}
{% else %}
  AND c.table_name NOT IN (SELECT x.table_name FROM all_all_tables x
                           WHERE x.owner = {{ bind("schema") }}
                             AND (x.nested = 'YES' OR x.iot_type IN ('IOT_OVERFLOW', 'IOT_MAPPING') OR x.secondary = 'Y' OR x.dropped = 'YES'))
{% endif %}
ORDER BY c.table_name, COALESCE(c.column_id, 100000 + c.internal_column_id)

-- [keys]
SELECT c.table_name AS table_name,
       c.constraint_name AS constraint_name,
       CASE c.constraint_type WHEN 'R' THEN 'F' ELSE c.constraint_type END AS constraint_type,
       cc.column_name AS column_name,
       cc.position AS position,
       CASE WHEN c.constraint_type = 'R' THEN c.r_owner END AS ref_schema,
       rc.table_name AS ref_table,
       rc.column_name AS ref_column,
       CASE WHEN c.constraint_type = 'R' THEN c.delete_rule END AS on_delete,
       CAST(NULL AS VARCHAR2(9)) AS on_update
FROM all_constraints c
JOIN all_cons_columns cc ON cc.owner = c.owner AND cc.constraint_name = c.constraint_name AND cc.table_name = c.table_name
LEFT JOIN all_cons_columns rc ON c.constraint_type = 'R' AND rc.owner = c.r_owner AND rc.constraint_name = c.r_constraint_name AND rc.position = cc.position
WHERE c.owner = {{ bind("schema") }}
  AND c.constraint_type IN ('P', 'U', 'R')
  AND c.table_name NOT LIKE 'BIN$%'
  AND c.constraint_name NOT LIKE 'SYS!_C!_SNAP$%' ESCAPE '!'
  AND cc.column_name NOT LIKE 'SYS!_NC%' ESCAPE '!'
{% if table %}
  AND c.table_name = {{ bind("table") }}
{% endif %}
ORDER BY c.table_name, c.constraint_name, cc.position

-- [checks]
SELECT c.table_name AS table_name,
       c.constraint_name AS constraint_name,
       c.search_condition AS expression
FROM all_constraints c
WHERE c.owner = {{ bind("schema") }}
  AND c.constraint_type = 'C'
  AND c.table_name NOT LIKE 'BIN$%'
{% if version >= 120200 %}
  AND (c.search_condition_vc IS NULL OR NOT REGEXP_LIKE(c.search_condition_vc, '^"[^"]+" IS NOT NULL$'))
{% endif %}
{% if table %}
  AND c.table_name = {{ bind("table") }}
{% endif %}
ORDER BY c.table_name, c.constraint_name

-- [indexes]
SELECT i.table_name AS table_name,
       i.index_name AS index_name,
       CASE WHEN i.uniqueness = 'UNIQUE' THEN 'Y' ELSE 'N' END AS is_unique,
       CASE WHEN i.index_type LIKE 'FUNCTION-BASED %' THEN SUBSTR(i.index_type, 16) ELSE i.index_type END AS method,
{% if version >= 120200 %}
       COALESCE(RTRIM(tc.data_default_vc, CHR(32) || CHR(9) || CHR(10) || CHR(13)), ic.column_name) AS column_expr,
{% else %}
       ic.column_name AS column_expr,
{% endif %}
       ic.column_position AS position,
       CASE WHEN ic.descend = 'DESC' THEN 'Y' ELSE 'N' END AS descending
{% if version < 120200 %}
       , ie.column_expression AS column_expression
{% endif %}
FROM all_indexes i
JOIN all_ind_columns ic ON ic.index_owner = i.owner AND ic.index_name = i.index_name
{% if version < 120200 %}
LEFT JOIN all_ind_expressions ie ON ie.index_owner = ic.index_owner AND ie.index_name = ic.index_name AND ie.column_position = ic.column_position
{% endif %}
{% if version >= 120200 %}
LEFT JOIN all_tab_cols tc ON tc.owner = i.table_owner AND tc.table_name = i.table_name AND tc.column_name = ic.column_name
     AND tc.virtual_column = 'YES' AND tc.hidden_column = 'YES'
{% endif %}
WHERE i.table_owner = {{ bind("schema") }}
  AND i.table_type = 'TABLE'
  AND i.index_name NOT LIKE 'BIN$%'
  AND i.table_name NOT LIKE 'BIN$%'
  AND i.index_type NOT IN ('LOB', 'IOT - TOP', 'IOT - NESTED')
  AND NVL(i.dropped, 'NO') = 'NO'
  AND NVL(i.secondary, 'N') = 'N'
  AND (i.owner, i.index_name) NOT IN (SELECT k.index_owner, k.index_name FROM all_constraints k
                                      WHERE k.owner = {{ bind("schema") }} AND k.constraint_type IN ('P', 'U')
                                        AND k.index_owner IS NOT NULL AND k.index_name IS NOT NULL
{% if table %}
                                        AND k.table_name = {{ bind("table") }}
{% endif %}
                                     )
{% if table %}
  AND i.table_name = {{ bind("table") }}
{% else %}
  AND i.table_name NOT IN (SELECT x.table_name FROM all_all_tables x
                           WHERE x.owner = {{ bind("schema") }}
                             AND (x.nested = 'YES' OR x.iot_type IN ('IOT_OVERFLOW', 'IOT_MAPPING') OR x.secondary = 'Y' OR x.dropped = 'YES'))
{% endif %}
ORDER BY i.table_name, i.index_name, ic.column_position

-- [triggers]
SELECT t.table_name AS table_name,
       t.trigger_name AS name,
       CASE WHEN t.trigger_type LIKE 'BEFORE%' THEN 'BEFORE'
            WHEN t.trigger_type LIKE 'AFTER%' THEN 'AFTER'
            WHEN t.trigger_type LIKE 'INSTEAD%' THEN 'INSTEAD OF'
            ELSE t.trigger_type END AS timing,
       REPLACE(t.triggering_event, ' OR ', ',') AS events,
       t.trigger_body AS definition
FROM all_triggers t
WHERE t.table_owner = {{ bind("schema") }}
  AND t.base_object_type IN ('TABLE', 'VIEW')
  AND t.trigger_name NOT LIKE 'BIN$%'
{% if table %}
  AND t.table_name = {{ bind("table") }}
{% endif %}
ORDER BY t.table_name, t.trigger_name

-- [view_definition]
SELECT v.text AS definition
FROM all_views v
WHERE v.owner = {{ bind("schema") }} AND v.view_name = {{ bind("name") }}
UNION ALL
SELECT m.query
FROM all_mviews m
WHERE m.owner = {{ bind("schema") }} AND m.mview_name = {{ bind("name") }}

-- [routine_source]
SELECT s.line AS line,
       CASE WHEN s.line = 1 THEN 'CREATE OR REPLACE ' || s.text ELSE s.text END AS text
FROM all_source s
WHERE s.owner = {{ bind("schema") }}
  AND s.name = {{ bind("name") }}
  AND s.type = {{ bind("kind") }}
ORDER BY s.line

-- [reserved_words]
SELECT DISTINCT r.keyword AS word
FROM v$reserved_words r
WHERE (r.reserved = 'Y' OR r.res_semi = 'Y' OR r.res_attr = 'Y')
  AND REGEXP_LIKE(r.keyword, '^[A-Z][A-Z0-9_$#]*$')
ORDER BY 1
