-- SQL Server：服务器级和当前库两级。id 带前缀：server:<登录名>、db:<用户或角色>。
-- 登录名到数据库用户的映射按 sid 记成一条成员关系，实际权限因此能从登录名走到库内角色。
-- 低权限账号查 sys.* 不报错，只是看不到别人，用 caps 段的 full_view 判断范围是否完整。

-- [caps]
SELECT CASE WHEN IS_SRVROLEMEMBER('sysadmin') = 1 OR HAS_PERMS_BY_NAME(NULL, NULL, 'VIEW ANY DEFINITION') = 1 THEN 1 ELSE 0 END AS full_view

-- [principals]
SELECT N'server:' + p.name AS id, p.name AS name, NULL AS host,
       CASE p.type WHEN 'R' THEN 'role' WHEN 'G' THEN 'group' ELSE 'login' END AS principal_kind,
       'server' AS principal_level,
       CASE WHEN p.type = 'R' THEN 0 WHEN p.is_disabled = 1 THEN 0 ELSE 1 END AS can_login,
       p.is_disabled AS locked,
       NULL AS expires_at,
       CASE WHEN p.is_fixed_role = 1 OR p.name LIKE N'##%' OR p.name LIKE N'NT %' OR p.name = N'public' THEN 1 ELSE 0 END AS builtin,
       p.type_desc AS attr_type,
       p.default_database_name AS attr_default_database
FROM sys.server_principals p
WHERE p.type IN ('S', 'U', 'G', 'R', 'E', 'X')
UNION ALL
SELECT N'db:' + p.name, p.name, NULL,
       CASE p.type WHEN 'R' THEN 'role' WHEN 'G' THEN 'group' ELSE 'user' END,
       'database',
       NULL, NULL, NULL,
       CASE WHEN p.is_fixed_role = 1 OR p.principal_id < 5 THEN 1 ELSE 0 END,
       p.type_desc, NULL
FROM sys.database_principals p
WHERE p.type IN ('S', 'U', 'G', 'R', 'E', 'X')
ORDER BY 5 DESC, 2

-- [memberships]
SELECT N'server:' + m.name AS member_id, N'server:' + r.name AS role_id, 0 AS admin_option, NULL AS inherits, NULL AS default_role
FROM sys.server_role_members x
JOIN sys.server_principals m ON m.principal_id = x.member_principal_id
JOIN sys.server_principals r ON r.principal_id = x.role_principal_id
UNION ALL
SELECT N'db:' + m.name, N'db:' + r.name, 0, NULL, NULL
FROM sys.database_role_members x
JOIN sys.database_principals m ON m.principal_id = x.member_principal_id
JOIN sys.database_principals r ON r.principal_id = x.role_principal_id
UNION ALL
SELECT N'server:' + s.name, N'db:' + d.name, 0, NULL, NULL
FROM sys.database_principals d
JOIN sys.server_principals s ON s.sid = d.sid
WHERE d.type IN ('S', 'U', 'G', 'E', 'X')

-- [grants_of]
SELECT N'server:' + g.name AS grantee, 'global' AS grant_scope, NULL AS db_name, NULL AS schema_name, NULL AS object_kind,
       NULL AS object_name, NULL AS column_name, p.permission_name AS privilege,
       CASE p.state WHEN 'D' THEN 'deny' ELSE 'grant' END AS effect,
       CASE WHEN p.state = 'W' THEN 1 ELSE 0 END AS grantable,
       r.name AS grantor
FROM sys.server_permissions p
JOIN sys.server_principals g ON g.principal_id = p.grantee_principal_id
LEFT JOIN sys.server_principals r ON r.principal_id = p.grantor_principal_id
WHERE p.class = 100 AND N'server:' + g.name = {{ bind("name") }}
UNION ALL
SELECT N'db:' + g.name,
       CASE p.class WHEN 0 THEN 'database' WHEN 3 THEN 'schema' ELSE CASE WHEN p.minor_id > 0 THEN 'column' ELSE 'object' END END,
       CASE WHEN p.class = 0 THEN DB_NAME() END,
       CASE p.class WHEN 3 THEN SCHEMA_NAME(p.major_id) WHEN 1 THEN OBJECT_SCHEMA_NAME(p.major_id) END,
       CASE WHEN p.class = 1 THEN LOWER(o.type_desc) END,
       CASE WHEN p.class = 1 THEN COALESCE(o.name, OBJECT_NAME(p.major_id)) END,
       c.name,
       p.permission_name,
       CASE p.state WHEN 'D' THEN 'deny' ELSE 'grant' END,
       CASE WHEN p.state = 'W' THEN 1 ELSE 0 END,
       r.name
FROM sys.database_permissions p
JOIN sys.database_principals g ON g.principal_id = p.grantee_principal_id
LEFT JOIN sys.database_principals r ON r.principal_id = p.grantor_principal_id
LEFT JOIN sys.all_objects o ON p.class = 1 AND o.object_id = p.major_id
LEFT JOIN sys.all_columns c ON p.class = 1 AND c.object_id = p.major_id AND c.column_id = p.minor_id
WHERE p.class IN (0, 1, 3) AND N'db:' + g.name = {{ bind("name") }}

-- [grants_on]
SELECT N'db:' + g.name AS grantee,
       CASE WHEN p.minor_id > 0 THEN 'column' ELSE 'object' END AS grant_scope,
       NULL AS db_name, OBJECT_SCHEMA_NAME(p.major_id) AS schema_name, 'table' AS object_kind,
       OBJECT_NAME(p.major_id) AS object_name, c.name AS column_name, p.permission_name AS privilege,
       CASE p.state WHEN 'D' THEN 'deny' ELSE 'grant' END AS effect,
       CASE WHEN p.state = 'W' THEN 1 ELSE 0 END AS grantable,
       r.name AS grantor
FROM sys.database_permissions p
JOIN sys.database_principals g ON g.principal_id = p.grantee_principal_id
LEFT JOIN sys.database_principals r ON r.principal_id = p.grantor_principal_id
LEFT JOIN sys.all_columns c ON c.object_id = p.major_id AND c.column_id = p.minor_id
WHERE p.class = 1 AND OBJECT_SCHEMA_NAME(p.major_id) = {{ bind("schema") }} AND OBJECT_NAME(p.major_id) = {{ bind("table") }}
UNION ALL
SELECT N'db:' + g.name, 'schema', NULL, SCHEMA_NAME(p.major_id), NULL, NULL, NULL, p.permission_name,
       CASE p.state WHEN 'D' THEN 'deny' ELSE 'grant' END, CASE WHEN p.state = 'W' THEN 1 ELSE 0 END, r.name
FROM sys.database_permissions p
JOIN sys.database_principals g ON g.principal_id = p.grantee_principal_id
LEFT JOIN sys.database_principals r ON r.principal_id = p.grantor_principal_id
WHERE p.class = 3 AND SCHEMA_NAME(p.major_id) = {{ bind("schema") }}
UNION ALL
SELECT N'db:' + g.name, 'database', DB_NAME(), NULL, NULL, NULL, NULL, p.permission_name,
       CASE p.state WHEN 'D' THEN 'deny' ELSE 'grant' END, CASE WHEN p.state = 'W' THEN 1 ELSE 0 END, r.name
FROM sys.database_permissions p
JOIN sys.database_principals g ON g.principal_id = p.grantee_principal_id
LEFT JOIN sys.database_principals r ON r.principal_id = p.grantor_principal_id
WHERE p.class = 0 AND p.permission_name IN ('SELECT', 'INSERT', 'UPDATE', 'DELETE', 'ALTER', 'CONTROL')
UNION ALL
SELECT N'db:' + f.name, 'database', DB_NAME(), NULL, NULL, NULL, NULL, f.priv, 'implicit', 0, NULL
FROM (VALUES (N'db_owner', 'CONTROL'), (N'db_datareader', 'SELECT'), (N'db_datawriter', 'INSERT, UPDATE, DELETE')) f(name, priv)
UNION ALL
SELECT N'server:sysadmin', 'global', NULL, NULL, NULL, NULL, NULL, 'SYSADMIN', 'implicit', 0, NULL
UNION ALL
SELECT N'db:' + o.name, 'object', NULL, s.name, 'table', {{ bind("table") }}, NULL, 'OWNER', 'implicit', 0, NULL
FROM sys.schemas s
JOIN sys.database_principals o ON o.principal_id = s.principal_id
WHERE s.name = {{ bind("schema") }}

-- [owned]
SELECT TOP 50 s.name AS object_name, 'schema' AS object_kind
FROM sys.schemas s
JOIN sys.database_principals p ON p.principal_id = s.principal_id
WHERE p.name = {{ bind("name") }}
