-- MariaDB：角色在 mysql.user 里 is_role = 'Y'，host 为空；GRANTEE 对角色写 'role'@''。
-- information_schema 的角色行含继承来的权限，下面用 mysql 库的授权表滤掉，来源路径才准。
-- 全局权限存成位掩码没法这样比，角色的全局权限行可能含继承。

-- [principals]
SELECT CONCAT('''', u.User, '''@''', u.Host, '''') AS id,
       u.User AS name,
       NULLIF(u.Host, '') AS host,
       CASE WHEN u.is_role = 'Y' THEN 'role' ELSE 'user' END AS principal_kind,
       'server' AS principal_level,
       CASE WHEN u.is_role = 'Y' OR JSON_VALUE(g.Priv, '$.account_locked') = 'true' THEN 0 ELSE 1 END AS can_login,
       CASE WHEN JSON_VALUE(g.Priv, '$.account_locked') = 'true' THEN 1 ELSE 0 END AS locked,
       NULL AS expires_at,
       CASE WHEN u.User IN ('mariadb.sys', 'PUBLIC') THEN 1 ELSE 0 END AS builtin,
       u.plugin AS attr_plugin,
       u.password_expired AS attr_password_expired,
       u.default_role AS attr_default_role
FROM mysql.user u
LEFT JOIN mysql.global_priv g ON g.User = u.User AND g.Host = u.Host
ORDER BY u.User, u.Host

-- [caps]

-- [memberships]
SELECT CONCAT('''', m.User, '''@''', m.Host, '''') AS member_id,
       CONCAT('''', m.Role, '''@''''') AS role_id,
       m.Admin_option AS admin_option,
       NULL AS inherits,
       CASE WHEN u.default_role = m.Role THEN 1 ELSE 0 END AS default_role
FROM mysql.roles_mapping m
LEFT JOIN mysql.user u ON u.User = m.User AND u.Host = m.Host

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
  AND EXISTS (SELECT 1 FROM mysql.db d
              WHERE CONCAT('''', d.User, '''@''', d.Host, '''') = p.GRANTEE AND d.Db = p.TABLE_SCHEMA)
UNION ALL
SELECT p.GRANTEE, 'object', NULL, p.TABLE_SCHEMA, 'table', p.TABLE_NAME, NULL, p.PRIVILEGE_TYPE, 'grant', p.IS_GRANTABLE, NULL
FROM information_schema.TABLE_PRIVILEGES p
WHERE p.GRANTEE = {{ bind("name") }}
  AND EXISTS (SELECT 1 FROM mysql.tables_priv t
              WHERE CONCAT('''', t.User, '''@''', t.Host, '''') = p.GRANTEE AND t.Db = p.TABLE_SCHEMA
                AND t.Table_name = p.TABLE_NAME AND FIND_IN_SET(p.PRIVILEGE_TYPE, UPPER(CAST(t.Table_priv AS CHAR))))
UNION ALL
SELECT p.GRANTEE, 'column', NULL, p.TABLE_SCHEMA, 'table', p.TABLE_NAME, p.COLUMN_NAME, p.PRIVILEGE_TYPE, 'grant', p.IS_GRANTABLE, NULL
FROM information_schema.COLUMN_PRIVILEGES p
WHERE p.GRANTEE = {{ bind("name") }}
  AND EXISTS (SELECT 1 FROM mysql.columns_priv c
              WHERE CONCAT('''', c.User, '''@''', c.Host, '''') = p.GRANTEE AND c.Db = p.TABLE_SCHEMA
                AND c.Table_name = p.TABLE_NAME AND c.Column_name = p.COLUMN_NAME
                AND FIND_IN_SET(p.PRIVILEGE_TYPE, UPPER(CAST(c.Column_priv AS CHAR))))
UNION ALL
SELECT CONCAT('''', r.User, '''@''', r.Host, ''''), 'object', NULL, r.Db, LOWER(r.Routine_type), r.Routine_name, NULL,
       UPPER(CAST(r.Proc_priv AS CHAR)), 'grant', 'NO', r.Grantor
FROM mysql.procs_priv r
WHERE CONCAT('''', r.User, '''@''', r.Host, '''') = {{ bind("name") }}

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
  AND EXISTS (SELECT 1 FROM mysql.db d
              WHERE CONCAT('''', d.User, '''@''', d.Host, '''') = p.GRANTEE AND d.Db = p.TABLE_SCHEMA)
UNION ALL
SELECT p.GRANTEE, 'object', NULL, p.TABLE_SCHEMA, 'table', p.TABLE_NAME, NULL, p.PRIVILEGE_TYPE, 'grant', p.IS_GRANTABLE, NULL
FROM information_schema.TABLE_PRIVILEGES p
WHERE p.TABLE_SCHEMA = {{ bind("schema") }} AND p.TABLE_NAME = {{ bind("table") }}
  AND EXISTS (SELECT 1 FROM mysql.tables_priv t
              WHERE CONCAT('''', t.User, '''@''', t.Host, '''') = p.GRANTEE AND t.Db = p.TABLE_SCHEMA
                AND t.Table_name = p.TABLE_NAME AND FIND_IN_SET(p.PRIVILEGE_TYPE, UPPER(CAST(t.Table_priv AS CHAR))))
UNION ALL
SELECT p.GRANTEE, 'column', NULL, p.TABLE_SCHEMA, 'table', p.TABLE_NAME, p.COLUMN_NAME, p.PRIVILEGE_TYPE, 'grant', p.IS_GRANTABLE, NULL
FROM information_schema.COLUMN_PRIVILEGES p
WHERE p.TABLE_SCHEMA = {{ bind("schema") }} AND p.TABLE_NAME = {{ bind("table") }}
  AND EXISTS (SELECT 1 FROM mysql.columns_priv c
              WHERE CONCAT('''', c.User, '''@''', c.Host, '''') = p.GRANTEE AND c.Db = p.TABLE_SCHEMA
                AND c.Table_name = p.TABLE_NAME AND c.Column_name = p.COLUMN_NAME
                AND FIND_IN_SET(p.PRIVILEGE_TYPE, UPPER(CAST(c.Column_priv AS CHAR))))

-- [grants_on@limited]
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
