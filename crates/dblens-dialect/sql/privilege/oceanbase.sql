-- OceanBase CE（MySQL 模式）：mysql.user 列名小写；没有 mysql.tables_priv，授权只取 information_schema。

-- [caps]

-- [principals]
SELECT CONCAT('''', u.user, '''@''', u.host, '''') AS id,
       u.user AS name,
       u.host AS host,
       CASE WHEN EXISTS (SELECT 1 FROM mysql.role_edges r WHERE r.FROM_USER = u.user AND r.FROM_HOST = u.host) THEN 'role' ELSE 'user' END AS principal_kind,
       'server' AS principal_level,
       CASE WHEN u.account_locked = 'Y' THEN 0 ELSE 1 END AS can_login,
       u.account_locked AS locked,
       NULL AS expires_at,
       CASE WHEN u.user IN ('root', 'proxyro', 'ocp_monitor') THEN 1 ELSE 0 END AS builtin,
       u.password_expired AS attr_password_expired
FROM mysql.user u
ORDER BY u.user, u.host

-- [memberships]
SELECT CONCAT('''', e.TO_USER, '''@''', e.TO_HOST, '''') AS member_id,
       CONCAT('''', e.FROM_USER, '''@''', e.FROM_HOST, '''') AS role_id,
       e.WITH_ADMIN_OPTION AS admin_option,
       NULL AS inherits,
       NULL AS default_role
FROM mysql.role_edges e

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
