-- GBase 8s (Informix family) metadata queries. tabid >= 100 are user objects, except the internal rows with a blank
-- tabtype (GL_COLLATE, GL_CTYPE, VERSION); names are fixed CHAR, so TRIM them.
-- Every query runs in the CURRENT database: the service connects to the catalog it wants.
-- Columns: type_text and nullable are placeholders; meta::coltype decodes coltype, collength and extended_type.
-- Key and index columns live in part1..part16 (negative = descending), expanded here by a template loop.
-- View, routine and check text is stored in CHAR chunks: one row per chunk, joined by the assembler.

-- [catalogs]
SELECT TRIM(name) AS name,
       CASE WHEN name IN ('sysmaster', 'sysadmin', 'sysuser', 'sysutils', 'gbasedbt') THEN 't' ELSE 'f' END::BOOLEAN AS is_system
FROM sysmaster:sysdatabases
{% if filter %}
WHERE UPPER(name) LIKE UPPER({{ bind("filter") }}) ESCAPE '!'
{% endif %}
ORDER BY 2, 1

-- [schemas]
SELECT DISTINCT TRIM(owner) AS name,
       CASE WHEN owner IN ('gbasedbt', 'informix') THEN 't' ELSE 'f' END::BOOLEAN AS is_system
FROM systables
WHERE tabtype <> ' '
ORDER BY 2, 1

-- [tables]
SELECT TRIM(t.tabname) AS name, c.comments AS comment, CAST(NULL AS VARCHAR(1)) AS partition
FROM systables t
LEFT JOIN syscomments c ON c.tabname = t.tabname
WHERE t.tabid >= 100 AND t.tabtype = 'T'
  AND t.owner = {{ bind("schema") }}
{% if table %}
  AND t.tabname = {{ bind("table") }}
{% endif %}
{% if filter %}
  AND (UPPER(t.tabname) LIKE UPPER({{ bind("filter") }}) ESCAPE '!'
       OR UPPER(c.comments) LIKE UPPER({{ bind("filter") }}) ESCAPE '!')
{% endif %}
ORDER BY 1

-- [views]
SELECT TRIM(t.tabname) AS name, c.comments AS comment
FROM systables t
LEFT JOIN syscomments c ON c.tabname = t.tabname
WHERE t.tabid >= 100 AND t.tabtype = 'V'
  AND t.owner = {{ bind("schema") }}
{% if filter %}
  AND UPPER(t.tabname) LIKE UPPER({{ bind("filter") }}) ESCAPE '!'
{% endif %}
ORDER BY 1

-- [sequences]
SELECT TRIM(t.tabname) AS name, s.start_val AS start_value, s.inc_val AS increment,
       s.min_val AS min_value, s.max_val AS max_value,
       CASE WHEN s.cycle = '1' THEN 't' ELSE 'f' END::BOOLEAN AS cycle, s.cache AS cache
FROM syssequences s
JOIN systables t ON t.tabid = s.tabid
WHERE t.owner = {{ bind("schema") }}
{% if filter %}
  AND UPPER(t.tabname) LIKE UPPER({{ bind("filter") }}) ESCAPE '!'
{% endif %}
ORDER BY 1

-- [routines]
SELECT TRIM(p.procname) AS name,
       CASE WHEN p.isproc = 't' THEN 'PROCEDURE' ELSE 'FUNCTION' END AS kind,
       CAST(NULL AS VARCHAR(1)) AS comment
FROM sysprocedures p
WHERE p.internal = 'f'
  AND p.owner = {{ bind("schema") }}
{% if filter %}
  AND UPPER(p.procname) LIKE UPPER({{ bind("filter") }}) ESCAPE '!'
{% endif %}
ORDER BY 1

-- [synonyms]
SELECT TRIM(t.tabname) AS name,
       COALESCE(TRIM(b.owner), TRIM(y.owner)) AS target_schema,
       COALESCE(TRIM(b.tabname), TRIM(y.tabname)) AS target_name,
       TRIM(y.dbname) AS db_link
FROM systables t
JOIN syssyntable y ON y.tabid = t.tabid
LEFT JOIN systables b ON b.tabid = y.btabid
WHERE t.tabtype IN ('S', 'P') AND t.owner = {{ bind("schema") }}
{% if filter %}
  AND UPPER(t.tabname) LIKE UPPER({{ bind("filter") }}) ESCAPE '!'
{% endif %}
ORDER BY 1

-- [columns]
SELECT TRIM(t.tabname) AS table_name, TRIM(c.colname) AS name, c.colno AS ordinal,
       CAST(NULL AS VARCHAR(1)) AS type_text, 't'::BOOLEAN AS nullable,
       c.coltype AS coltype, c.collength AS collength, TRIM(x.name) AS extended_type,
       CASE d.type WHEN 'L' THEN TRIM(d.default) WHEN 'C' THEN 'CURRENT' WHEN 'T' THEN 'TODAY'
                   WHEN 'U' THEN 'USER' END AS default_value,
       m.comments AS comment,
       CASE WHEN MOD(c.coltype, 256) IN (6, 18, 53) THEN 't' ELSE 'f' END::BOOLEAN AS auto_increment,
       CAST(NULL AS VARCHAR(1)) AS generated
FROM systables t
JOIN syscolumns c ON c.tabid = t.tabid
LEFT JOIN sysxtdtypes x ON x.extended_id = c.extended_id AND c.extended_id > 0
LEFT JOIN sysdefaults d ON d.tabid = c.tabid AND d.colno = c.colno
LEFT JOIN syscolcomments m ON m.tabname = t.tabname AND m.colname = c.colname
WHERE t.tabid >= 100 AND t.owner = {{ bind("schema") }}
{% if table %}
  AND t.tabname = {{ bind("table") }}
{% endif %}
ORDER BY 1, 3

-- [keys]
{% for p in range(1, 17) %}
{% if not loop.first %}UNION ALL {% endif %}SELECT TRIM(t.tabname) AS table_name, TRIM(k.constrname) AS constraint_name,
       CASE k.constrtype WHEN 'P' THEN 'P' WHEN 'U' THEN 'U' ELSE 'F' END AS constraint_type,
       TRIM(c.colname) AS column_name, {{ p }} AS position,
       TRIM(rt.owner) AS ref_schema, TRIM(rt.tabname) AS ref_table, TRIM(rc.colname) AS ref_column,
       CASE WHEN k.constrtype <> 'R' THEN NULL WHEN r.delrule = 'C' THEN 'CASCADE' ELSE 'NO ACTION' END AS on_delete,
       CASE WHEN k.constrtype <> 'R' THEN NULL ELSE 'NO ACTION' END AS on_update
FROM sysconstraints k
JOIN systables t ON t.tabid = k.tabid
JOIN sysindexes x ON x.idxname = k.idxname AND x.tabid = k.tabid
JOIN syscolumns c ON c.tabid = k.tabid AND c.colno = ABS(x.part{{ p }})
LEFT JOIN sysreferences r ON r.constrid = k.constrid
LEFT JOIN systables rt ON rt.tabid = r.ptabid
LEFT JOIN sysconstraints pk ON pk.constrid = r."primary"
LEFT JOIN sysindexes px ON px.idxname = pk.idxname AND px.tabid = pk.tabid
LEFT JOIN syscolumns rc ON rc.tabid = r.ptabid AND rc.colno = ABS(px.part{{ p }})
WHERE k.constrtype IN ('P', 'U', 'R') AND t.owner = {{ bind("schema") }}
{% if table %}
  AND t.tabname = {{ bind("table") }}
{% endif %}
{% endfor %}
ORDER BY 1, 2, 5

-- [checks]
SELECT TRIM(t.tabname) AS table_name, TRIM(k.constrname) AS constraint_name, c.checktext AS expression
FROM sysconstraints k
JOIN systables t ON t.tabid = k.tabid
JOIN syschecks c ON c.constrid = k.constrid AND c.type = 'T'
WHERE k.constrtype = 'C' AND t.owner = {{ bind("schema") }}
{% if table %}
  AND t.tabname = {{ bind("table") }}
{% endif %}
ORDER BY 1, 2, c.seqno

-- [indexes]
-- Indexes that back a constraint are excluded: they appear as keys.
{% for p in range(1, 17) %}
{% if not loop.first %}UNION ALL {% endif %}SELECT TRIM(t.tabname) AS table_name, TRIM(x.idxname) AS index_name,
       CASE WHEN x.idxtype = 'U' THEN 't' ELSE 'f' END::BOOLEAN AS is_unique,
       CAST(NULL AS VARCHAR(1)) AS method, TRIM(c.colname) AS column_expr, {{ p }} AS position,
       CASE WHEN x.part{{ p }} < 0 THEN 't' ELSE 'f' END::BOOLEAN AS descending
FROM sysindexes x
JOIN systables t ON t.tabid = x.tabid
JOIN syscolumns c ON c.tabid = x.tabid AND c.colno = ABS(x.part{{ p }})
WHERE t.tabid >= 100 AND t.owner = {{ bind("schema") }}
  AND NOT EXISTS (SELECT 1 FROM sysconstraints k WHERE k.idxname = x.idxname AND k.tabid = x.tabid)
{% if table %}
  AND t.tabname = {{ bind("table") }}
{% endif %}
{% endfor %}
ORDER BY 1, 2, 6

-- [triggers]
SELECT TRIM(t.tabname) AS table_name, TRIM(g.trigname) AS name, CAST(NULL AS VARCHAR(1)) AS timing,
       CASE g.event WHEN 'I' THEN 'INSERT' WHEN 'U' THEN 'UPDATE' WHEN 'D' THEN 'DELETE' ELSE 'SELECT' END AS events,
       CAST(NULL AS VARCHAR(1)) AS definition
FROM systriggers g
JOIN systables t ON t.tabid = g.tabid
WHERE g.owner = {{ bind("schema") }}
{% if table %}
  AND t.tabname = {{ bind("table") }}
{% endif %}
ORDER BY 1, 2

-- [view_definition]
SELECT v.viewtext AS definition
FROM sysviews v
JOIN systables t ON t.tabid = v.tabid
WHERE t.owner = {{ bind("schema") }} AND t.tabname = {{ bind("name") }}
ORDER BY v.seqno

-- [routine_source]
SELECT b.seqno AS line, b.data AS text
FROM sysprocbody b
JOIN sysprocedures p ON p.procid = b.procid
WHERE b.datakey = 'T' AND p.owner = {{ bind("schema") }} AND p.procname = {{ bind("name") }}
{% if kind %}
  AND (CASE WHEN p.isproc = 't' THEN 'PROCEDURE' ELSE 'FUNCTION' END) = {{ bind("kind") }}
{% endif %}
ORDER BY b.seqno

-- [table_storage]
SELECT CAST(NULL AS VARCHAR(1)) AS engine, CAST(NULL AS VARCHAR(1)) AS charset, CAST(NULL AS VARCHAR(1)) AS collation,
       CAST(NULL AS VARCHAR(1)) AS tablespace, CAST(t.nrows AS BIGINT) AS row_count
FROM systables t
WHERE t.owner = {{ bind("schema") }} AND t.tabname = {{ bind("table") }}
