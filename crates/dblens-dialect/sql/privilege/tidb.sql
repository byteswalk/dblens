-- TiDB：没有 mysql.procs_priv，授权只取 information_schema。

-- [caps]

-- [grants_of]
SELECT p.GRANTEE AS grantee, 'global' AS grant_scope, NULL AS db_name, NULL AS schema_name, NULL AS object_kind,
       NULL AS object_name, NULL AS column_name, p.PRIVILEGE_TYPE AS privilege, 'grant' AS effect,
       p.IS_GRANTABLE AS grantable, NULL AS grantor
FROM information_schema.USER_PRIVILEGES p
WHERE p.GRANTEE = {{ bind("name") }} AND p.PRIVILEGE_TYPE <> 'USAGE'
UNION ALL
SELECT p.GRANTEE, 'schema', NULL, p.TABLE_SCHEMA, NULL, NULL, NULL, p.PRIVILEGE_TYPE, 'grant', p.IS_GRANTABLE, NULL
FROM information_schema.SCHEMA_PRIVILEGES p
WHERE p.GRANTEE = {{ bind("name") }}
UNION ALL
SELECT p.GRANTEE, 'object', NULL, p.TABLE_SCHEMA, 'table', p.TABLE_NAME, NULL, p.PRIVILEGE_TYPE, 'grant', p.IS_GRANTABLE, NULL
FROM information_schema.TABLE_PRIVILEGES p
WHERE p.GRANTEE = {{ bind("name") }}
UNION ALL
SELECT p.GRANTEE, 'column', NULL, p.TABLE_SCHEMA, 'table', p.TABLE_NAME, p.COLUMN_NAME, p.PRIVILEGE_TYPE, 'grant', p.IS_GRANTABLE, NULL
FROM information_schema.COLUMN_PRIVILEGES p
WHERE p.GRANTEE = {{ bind("name") }}
