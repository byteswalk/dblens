-- 账号与权限（M9a）。列名约定见 crates/dblens-dialect/src/privilege/mod.rs。
-- 主体 id 与 information_schema 的 GRANTEE 同形：'user'@'host'。@limited 段在主查询报 SQL 错误时使用。

-- [caps]
SELECT 1 AS full_view, @@partial_revokes AS partial_revokes

-- [principals]
SELECT CONCAT('''', u.User, '''@''', u.Host, '''') AS id,
       u.User AS name,
       u.Host AS host,
       CASE WHEN EXISTS (SELECT 1 FROM mysql.role_edges r WHERE r.FROM_USER = u.User AND r.FROM_HOST = u.Host)
                 OR (u.account_locked = 'Y' AND u.authentication_string = '') THEN 'role' ELSE 'user' END AS principal_kind,
       'server' AS principal_level,
       CASE WHEN u.account_locked = 'Y' THEN 0 ELSE 1 END AS can_login,
       u.account_locked AS locked,
       NULL AS expires_at,
       CASE WHEN u.User IN ('mysql.infoschema', 'mysql.session', 'mysql.sys') THEN 1 ELSE 0 END AS builtin,
       u.plugin AS attr_plugin,
       u.password_expired AS attr_password_expired
FROM mysql.user u
ORDER BY u.User, u.Host

-- [principals@limited]
SELECT DISTINCT p.GRANTEE AS id,
       SUBSTRING_INDEX(TRIM(LEADING '''' FROM p.GRANTEE), '''@''', 1) AS name,
       TRIM(TRAILING '''' FROM SUBSTRING_INDEX(p.GRANTEE, '''@''', -1)) AS host,
       'user' AS principal_kind,
       'server' AS principal_level,
       NULL AS can_login,
       NULL AS locked,
       NULL AS expires_at,
       0 AS builtin
FROM information_schema.USER_PRIVILEGES p
ORDER BY p.GRANTEE

-- [memberships]
SELECT CONCAT('''', e.TO_USER, '''@''', e.TO_HOST, '''') AS member_id,
       CONCAT('''', e.FROM_USER, '''@''', e.FROM_HOST, '''') AS role_id,
       e.WITH_ADMIN_OPTION AS admin_option,
       NULL AS inherits,
       CASE WHEN d.USER IS NULL THEN 0 ELSE 1 END AS default_role
FROM mysql.role_edges e
LEFT JOIN mysql.default_roles d
       ON d.USER = e.TO_USER AND d.HOST = e.TO_HOST
      AND d.DEFAULT_ROLE_USER = e.FROM_USER AND d.DEFAULT_ROLE_HOST = e.FROM_HOST

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
UNION ALL
SELECT CONCAT('''', r.User, '''@''', r.Host, ''''), 'object', NULL, r.Db, LOWER(r.Routine_type), r.Routine_name, NULL,
       UPPER(CAST(r.Proc_priv AS CHAR)), 'grant', 'NO', r.Grantor
FROM mysql.procs_priv r
WHERE CONCAT('''', r.User, '''@''', r.Host, '''') = {{ bind("name") }}

-- [grants_of@limited]
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

-- [grants_on]
SELECT p.GRANTEE AS grantee, 'global' AS grant_scope, NULL AS db_name, NULL AS schema_name, NULL AS object_kind,
       NULL AS object_name, NULL AS column_name, p.PRIVILEGE_TYPE AS privilege, 'grant' AS effect,
       p.IS_GRANTABLE AS grantable, NULL AS grantor
FROM information_schema.USER_PRIVILEGES p
WHERE p.PRIVILEGE_TYPE IN ('SELECT', 'INSERT', 'UPDATE', 'DELETE', 'ALTER', 'DROP', 'INDEX', 'REFERENCES', 'TRIGGER')
UNION ALL
SELECT p.GRANTEE, 'schema', NULL, p.TABLE_SCHEMA, NULL, NULL, NULL, p.PRIVILEGE_TYPE, 'grant', p.IS_GRANTABLE, NULL
FROM information_schema.SCHEMA_PRIVILEGES p
WHERE {{ bind("schema") }} LIKE p.TABLE_SCHEMA
  AND p.PRIVILEGE_TYPE IN ('SELECT', 'INSERT', 'UPDATE', 'DELETE', 'ALTER', 'DROP', 'INDEX', 'REFERENCES', 'TRIGGER')
UNION ALL
SELECT p.GRANTEE, 'object', NULL, p.TABLE_SCHEMA, 'table', p.TABLE_NAME, NULL, p.PRIVILEGE_TYPE, 'grant', p.IS_GRANTABLE, NULL
FROM information_schema.TABLE_PRIVILEGES p
WHERE p.TABLE_SCHEMA = {{ bind("schema") }} AND p.TABLE_NAME = {{ bind("table") }}
UNION ALL
SELECT p.GRANTEE, 'column', NULL, p.TABLE_SCHEMA, 'table', p.TABLE_NAME, p.COLUMN_NAME, p.PRIVILEGE_TYPE, 'grant', p.IS_GRANTABLE, NULL
FROM information_schema.COLUMN_PRIVILEGES p
WHERE p.TABLE_SCHEMA = {{ bind("schema") }} AND p.TABLE_NAME = {{ bind("table") }}
