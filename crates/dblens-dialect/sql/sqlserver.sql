-- SQL Server metadata queries (catalog views only). Base: SQL Server 2017+ (STRING_AGG); tested on 2022.
-- Every query runs in the CURRENT database: the service switches with USE [catalog] first (catalogs lists them).
-- Booleans are returned as bit. Microsoft-shipped objects are excluded.
-- Comments are the MS_Description extended property.

-- [catalogs]
-- Online databases the login can open; master/tempdb/model/msdb (database_id <= 4) are system.
SELECT d.name AS name,
       CAST(CASE WHEN d.database_id <= 4 THEN 1 ELSE 0 END AS bit) AS is_system
FROM sys.databases d
WHERE d.state = 0
  AND HAS_DBACCESS(d.name) = 1
{% if filter %}
  AND UPPER(d.name) LIKE UPPER({{ bind("filter") }}) ESCAPE '!'
{% endif %}
ORDER BY CASE WHEN d.database_id <= 4 THEN 1 ELSE 0 END, d.name

-- [schemas]
-- sys, INFORMATION_SCHEMA, guest and the fixed database-role schemas (db_owner … db_denydatawriter) are system.
SELECT s.name AS name,
       CAST(CASE WHEN s.name IN ('sys', 'INFORMATION_SCHEMA', 'guest') OR s.schema_id BETWEEN 16384 AND 16399
                 THEN 1 ELSE 0 END AS bit) AS is_system
FROM sys.schemas s
ORDER BY s.name

-- [tables]
SELECT t.name AS name,
       CAST(ep.value AS nvarchar(max)) AS comment,
       CAST(NULL AS nvarchar(max)) AS partition
FROM sys.tables t
LEFT JOIN sys.extended_properties ep
       ON ep.class = 1 AND ep.major_id = t.object_id AND ep.minor_id = 0 AND ep.name = 'MS_Description'
WHERE t.schema_id = SCHEMA_ID({{ bind("schema") }})
  AND t.is_ms_shipped = 0
{% if table %}
  AND t.name = {{ bind("table") }}
{% endif %}
{% if filter %}
  AND (UPPER(t.name) LIKE UPPER({{ bind("filter") }}) ESCAPE '!'
       OR UPPER(CAST(ep.value AS nvarchar(max))) LIKE UPPER({{ bind("filter") }}) ESCAPE '!')
{% endif %}
ORDER BY t.name

-- [views]
SELECT v.name AS name,
       CAST(ep.value AS nvarchar(max)) AS comment
FROM sys.views v
LEFT JOIN sys.extended_properties ep
       ON ep.class = 1 AND ep.major_id = v.object_id AND ep.minor_id = 0 AND ep.name = 'MS_Description'
WHERE v.schema_id = SCHEMA_ID({{ bind("schema") }})
  AND v.is_ms_shipped = 0
{% if filter %}
  AND (UPPER(v.name) LIKE UPPER({{ bind("filter") }}) ESCAPE '!'
       OR UPPER(CAST(ep.value AS nvarchar(max))) LIKE UPPER({{ bind("filter") }}) ESCAPE '!')
{% endif %}
ORDER BY v.name

-- [sequences]
-- start_value etc. are sql_variant: cast to text.
SELECT s.name AS name,
       CAST(s.start_value AS nvarchar(40)) AS start_value,
       CAST(s.increment AS nvarchar(40)) AS increment,
       CAST(s.minimum_value AS nvarchar(40)) AS min_value,
       CAST(s.maximum_value AS nvarchar(40)) AS max_value,
       s.is_cycling AS cycle,
       CASE WHEN s.is_cached = 1 THEN s.cache_size END AS cache
FROM sys.sequences s
WHERE s.schema_id = SCHEMA_ID({{ bind("schema") }})
{% if filter %}
  AND UPPER(s.name) LIKE UPPER({{ bind("filter") }}) ESCAPE '!'
{% endif %}
ORDER BY s.name

-- [routines]
-- P/PC = procedures (T-SQL / CLR); FN/IF/TF/FS/FT = scalar, inline, table-valued and CLR functions.
SELECT o.name AS name,
       CASE WHEN o.type IN ('P', 'PC') THEN 'PROCEDURE' ELSE 'FUNCTION' END AS kind,
       CAST(ep.value AS nvarchar(max)) AS comment
FROM sys.objects o
LEFT JOIN sys.extended_properties ep
       ON ep.class = 1 AND ep.major_id = o.object_id AND ep.minor_id = 0 AND ep.name = 'MS_Description'
WHERE o.schema_id = SCHEMA_ID({{ bind("schema") }})
  AND o.type IN ('P', 'PC', 'FN', 'IF', 'TF', 'FS', 'FT')
  AND o.is_ms_shipped = 0
{% if filter %}
  AND (UPPER(o.name) LIKE UPPER({{ bind("filter") }}) ESCAPE '!'
       OR UPPER(CAST(ep.value AS nvarchar(max))) LIKE UPPER({{ bind("filter") }}) ESCAPE '!')
{% endif %}
ORDER BY o.name

-- [synonyms]
-- base_object_name is [server].[database].[schema].[object] with the leading parts optional.
SELECT s.name AS name,
       PARSENAME(s.base_object_name, 2) AS target_schema,
       PARSENAME(s.base_object_name, 1) AS target_name,
       PARSENAME(s.base_object_name, 4) AS db_link
FROM sys.synonyms s
WHERE s.schema_id = SCHEMA_ID({{ bind("schema") }})
{% if filter %}
  AND UPPER(s.name) LIKE UPPER({{ bind("filter") }}) ESCAPE '!'
{% endif %}
ORDER BY s.name

-- [columns]
-- type_text rebuilds the declared type: n(var)char lengths are in bytes in sys.columns (halved), -1 is max.
SELECT o.name AS table_name,
       c.name AS name,
       CAST(ROW_NUMBER() OVER (PARTITION BY o.object_id ORDER BY c.column_id) AS int) AS ordinal,
       ty.name + CASE
         WHEN ty.name IN ('varchar', 'char', 'varbinary', 'binary')
           THEN '(' + CASE WHEN c.max_length = -1 THEN 'max' ELSE CAST(c.max_length AS varchar(10)) END + ')'
         WHEN ty.name IN ('nvarchar', 'nchar')
           THEN '(' + CASE WHEN c.max_length = -1 THEN 'max' ELSE CAST(c.max_length / 2 AS varchar(10)) END + ')'
         WHEN ty.name IN ('decimal', 'numeric')
           THEN '(' + CAST(c.precision AS varchar(10)) + ',' + CAST(c.scale AS varchar(10)) + ')'
         WHEN ty.name IN ('datetime2', 'time', 'datetimeoffset')
           THEN '(' + CAST(c.scale AS varchar(10)) + ')'
         ELSE '' END AS type_text,
       c.is_nullable AS nullable,
       dc.definition AS default_value,
       CAST(ep.value AS nvarchar(max)) AS comment,
       c.is_identity AS auto_increment,
       cc.definition AS generated
FROM sys.objects o
JOIN sys.columns c ON c.object_id = o.object_id
JOIN sys.types ty ON ty.user_type_id = c.user_type_id
LEFT JOIN sys.default_constraints dc ON dc.object_id = c.default_object_id
LEFT JOIN sys.computed_columns cc ON cc.object_id = c.object_id AND cc.column_id = c.column_id
LEFT JOIN sys.extended_properties ep
       ON ep.class = 1 AND ep.major_id = c.object_id AND ep.minor_id = c.column_id AND ep.name = 'MS_Description'
WHERE o.schema_id = SCHEMA_ID({{ bind("schema") }})
  AND o.type IN ('U', 'V')
  AND o.is_ms_shipped = 0
{% if table %}
  AND o.name = {{ bind("table") }}
{% endif %}
ORDER BY o.name, c.column_id

-- [keys]
-- Primary keys and unique constraints (through their index), then foreign keys.
SELECT t.name AS table_name,
       kc.name AS constraint_name,
       CASE kc.type WHEN 'PK' THEN 'P' ELSE 'U' END AS constraint_type,
       c.name AS column_name,
       CAST(ic.key_ordinal AS int) AS position,
       CAST(NULL AS sysname) AS ref_schema,
       CAST(NULL AS sysname) AS ref_table,
       CAST(NULL AS sysname) AS ref_column,
       CAST(NULL AS nvarchar(20)) AS on_delete,
       CAST(NULL AS nvarchar(20)) AS on_update
FROM sys.key_constraints kc
JOIN sys.tables t ON t.object_id = kc.parent_object_id
JOIN sys.index_columns ic ON ic.object_id = kc.parent_object_id AND ic.index_id = kc.unique_index_id
JOIN sys.columns c ON c.object_id = ic.object_id AND c.column_id = ic.column_id
WHERE t.schema_id = SCHEMA_ID({{ bind("schema") }})
  AND ic.is_included_column = 0
{% if table %}
  AND t.name = {{ bind("table") }}
{% endif %}
UNION ALL
SELECT t.name,
       fk.name,
       'F',
       c.name,
       CAST(fkc.constraint_column_id AS int),
       SCHEMA_NAME(rt.schema_id),
       rt.name,
       rc.name,
       REPLACE(fk.delete_referential_action_desc, '_', ' '),
       REPLACE(fk.update_referential_action_desc, '_', ' ')
FROM sys.foreign_keys fk
JOIN sys.tables t ON t.object_id = fk.parent_object_id
JOIN sys.foreign_key_columns fkc ON fkc.constraint_object_id = fk.object_id
JOIN sys.columns c ON c.object_id = fkc.parent_object_id AND c.column_id = fkc.parent_column_id
JOIN sys.tables rt ON rt.object_id = fk.referenced_object_id
JOIN sys.columns rc ON rc.object_id = fkc.referenced_object_id AND rc.column_id = fkc.referenced_column_id
WHERE t.schema_id = SCHEMA_ID({{ bind("schema") }})
{% if table %}
  AND t.name = {{ bind("table") }}
{% endif %}
ORDER BY 1, 2, 5

-- [checks]
SELECT t.name AS table_name,
       ck.name AS constraint_name,
       ck.definition AS expression
FROM sys.check_constraints ck
JOIN sys.tables t ON t.object_id = ck.parent_object_id
WHERE t.schema_id = SCHEMA_ID({{ bind("schema") }})
{% if table %}
  AND t.name = {{ bind("table") }}
{% endif %}
ORDER BY t.name, ck.name

-- [indexes]
-- Key columns only (INCLUDE columns are skipped); heaps (index_id 0) and indexes backing PK/UNIQUE are excluded.
-- method is CLUSTERED / NONCLUSTERED / CLUSTERED COLUMNSTORE / …
SELECT t.name AS table_name,
       i.name AS index_name,
       i.is_unique AS is_unique,
       i.type_desc AS method,
       c.name AS column_expr,
       CAST(ic.key_ordinal AS int) AS position,
       ic.is_descending_key AS descending
FROM sys.indexes i
JOIN sys.tables t ON t.object_id = i.object_id
JOIN sys.index_columns ic ON ic.object_id = i.object_id AND ic.index_id = i.index_id
JOIN sys.columns c ON c.object_id = ic.object_id AND c.column_id = ic.column_id
WHERE t.schema_id = SCHEMA_ID({{ bind("schema") }})
  AND i.index_id > 0
  AND i.is_primary_key = 0
  AND i.is_unique_constraint = 0
  AND i.is_hypothetical = 0
  AND ic.is_included_column = 0
  AND ic.key_ordinal > 0
{% if table %}
  AND t.name = {{ bind("table") }}
{% endif %}
ORDER BY t.name, i.name, ic.key_ordinal

-- [triggers]
-- DML triggers on tables and views; definition is the full CREATE TRIGGER text.
SELECT o.name AS table_name,
       tr.name AS name,
       CASE WHEN tr.is_instead_of_trigger = 1 THEN 'INSTEAD OF' ELSE 'AFTER' END AS timing,
       (SELECT STRING_AGG(te.type_desc, ',') FROM sys.trigger_events te WHERE te.object_id = tr.object_id) AS events,
       m.definition AS definition
FROM sys.triggers tr
JOIN sys.objects o ON o.object_id = tr.parent_id
LEFT JOIN sys.sql_modules m ON m.object_id = tr.object_id
WHERE o.schema_id = SCHEMA_ID({{ bind("schema") }})
  AND tr.parent_class = 1
  AND tr.is_ms_shipped = 0
{% if table %}
  AND o.name = {{ bind("table") }}
{% endif %}
ORDER BY o.name, tr.name

-- [view_definition]
-- The module text is the full CREATE VIEW statement.
SELECT m.definition AS definition
FROM sys.views v
JOIN sys.sql_modules m ON m.object_id = v.object_id
WHERE v.schema_id = SCHEMA_ID({{ bind("schema") }})
  AND v.name = {{ bind("name") }}

-- [routine_source]
-- One row per routine: the full CREATE PROCEDURE / FUNCTION text.
SELECT 1 AS line,
       m.definition AS text
FROM sys.objects o
JOIN sys.sql_modules m ON m.object_id = o.object_id
WHERE o.schema_id = SCHEMA_ID({{ bind("schema") }})
  AND o.name = {{ bind("name") }}
{% if kind %}
  AND (CASE WHEN o.type IN ('P', 'PC') THEN 'PROCEDURE' ELSE 'FUNCTION' END) = {{ bind("kind") }}
{% endif %}

-- [referencing_keys]
-- Foreign keys of OTHER tables that reference this table.
SELECT SCHEMA_NAME(t.schema_id) AS table_schema,
       t.name AS table_name,
       fk.name AS constraint_name,
       c.name AS column_name,
       CAST(fkc.constraint_column_id AS int) AS position,
       rc.name AS ref_column,
       REPLACE(fk.delete_referential_action_desc, '_', ' ') AS on_delete,
       REPLACE(fk.update_referential_action_desc, '_', ' ') AS on_update
FROM sys.foreign_keys fk
JOIN sys.tables t ON t.object_id = fk.parent_object_id
JOIN sys.foreign_key_columns fkc ON fkc.constraint_object_id = fk.object_id
JOIN sys.columns c ON c.object_id = fkc.parent_object_id AND c.column_id = fkc.parent_column_id
JOIN sys.columns rc ON rc.object_id = fkc.referenced_object_id AND rc.column_id = fkc.referenced_column_id
WHERE fk.referenced_object_id = OBJECT_ID(QUOTENAME({{ bind("schema") }}) + '.' + QUOTENAME({{ bind("table") }}))
  AND fk.parent_object_id <> fk.referenced_object_id
ORDER BY 1, 2, 3, 5

-- [dependent_views]
-- Views whose definition references this table (schema-bound or not).
SELECT DISTINCT SCHEMA_NAME(v.schema_id) AS schema_name,
       v.name AS name,
       'VIEW' AS kind
FROM sys.sql_expression_dependencies d
JOIN sys.views v ON v.object_id = d.referencing_id
WHERE d.referenced_id = OBJECT_ID(QUOTENAME({{ bind("schema") }}) + '.' + QUOTENAME({{ bind("table") }}))
ORDER BY 1, 2

-- [grants]
-- Object-level permissions granted (G) or granted WITH GRANT OPTION (W); denials are not grants.
SELECT pr.name AS grantee,
       p.permission_name AS privilege,
       CAST(CASE WHEN p.state = 'W' THEN 1 ELSE 0 END AS bit) AS grantable
FROM sys.database_permissions p
JOIN sys.database_principals pr ON pr.principal_id = p.grantee_principal_id
WHERE p.class = 1
  AND p.minor_id = 0
  AND p.state IN ('G', 'W')
  AND p.major_id = OBJECT_ID(QUOTENAME({{ bind("schema") }}) + '.' + QUOTENAME({{ bind("table") }}))
ORDER BY 1, 2

-- [table_storage]
-- tablespace = filegroup of the heap / clustered index; row_count from partition metadata (no scan).
SELECT CAST(NULL AS nvarchar(10)) AS engine,
       CAST(NULL AS nvarchar(10)) AS charset,
       CAST(NULL AS nvarchar(10)) AS collation,
       (SELECT TOP 1 ds.name FROM sys.indexes i JOIN sys.data_spaces ds ON ds.data_space_id = i.data_space_id
        WHERE i.object_id = t.object_id AND i.index_id IN (0, 1)) AS tablespace,
       (SELECT SUM(p.rows) FROM sys.partitions p WHERE p.object_id = t.object_id AND p.index_id IN (0, 1)) AS row_count
FROM sys.tables t
WHERE t.schema_id = SCHEMA_ID({{ bind("schema") }})
  AND t.name = {{ bind("table") }}
