-- Oracle。主体 id 就是用户名或角色名；授权范围：系统权限 global，对象 object，列 column。

-- [principals]
SELECT u.username AS id, u.username AS name, NULL AS host, 'user' AS principal_kind, 'server' AS principal_level,
       NULL AS can_login,
       CASE WHEN u.account_status LIKE '%LOCKED%' THEN 1 ELSE 0 END AS locked,
       TO_CHAR(u.expiry_date, 'YYYY-MM-DD HH24:MI:SS') AS expires_at,
{% if version >= 120000 %}
       CASE WHEN u.oracle_maintained = 'Y' THEN 1 ELSE 0 END AS builtin,
{% else %}
       0 AS builtin,
{% endif %}
       u.account_status AS attr_status,
       u.profile AS attr_profile,
       u.default_tablespace AS attr_default_tablespace
FROM dba_users u
UNION ALL
SELECT r.role, r.role, NULL, 'role', 'server', NULL, NULL, NULL,
{% if version >= 120000 %}
       CASE WHEN r.oracle_maintained = 'Y' THEN 1 ELSE 0 END,
{% else %}
       0,
{% endif %}
       NULL, NULL, NULL
FROM dba_roles r
ORDER BY 2

-- [principals@limited]
SELECT u.username AS id, u.username AS name, NULL AS host, 'user' AS principal_kind, 'server' AS principal_level,
       NULL AS can_login, NULL AS locked, NULL AS expires_at, 0 AS builtin
FROM all_users u
UNION ALL
SELECT r.granted_role, r.granted_role, NULL, 'role', 'server', NULL, NULL, NULL, 0
FROM user_role_privs r
ORDER BY 2

-- [memberships]
SELECT p.grantee AS member_id, p.granted_role AS role_id, p.admin_option AS admin_option, NULL AS inherits,
       p.default_role AS default_role
FROM dba_role_privs p

-- [memberships@limited]
SELECT p.username AS member_id, p.granted_role AS role_id, p.admin_option AS admin_option, NULL AS inherits,
       p.default_role AS default_role
FROM user_role_privs p
UNION ALL
SELECT p.role, p.granted_role, p.admin_option, NULL, NULL
FROM role_role_privs p

-- [grants_of]
SELECT p.grantee AS grantee, 'global' AS grant_scope, NULL AS db_name, NULL AS schema_name, NULL AS object_kind,
       NULL AS object_name, NULL AS column_name, p.privilege AS privilege, 'grant' AS effect,
       p.admin_option AS grantable, NULL AS grantor
FROM dba_sys_privs p
WHERE p.grantee = {{ bind("name") }}
UNION ALL
SELECT p.grantee, 'object', NULL, p.owner, LOWER(p.type), p.table_name, NULL, p.privilege, 'grant', p.grantable, p.grantor
FROM dba_tab_privs p
WHERE p.grantee = {{ bind("name") }}
UNION ALL
SELECT p.grantee, 'column', NULL, p.owner, 'table', p.table_name, p.column_name, p.privilege, 'grant', p.grantable, p.grantor
FROM dba_col_privs p
WHERE p.grantee = {{ bind("name") }}

-- [grants_of@limited]
SELECT p.username AS grantee, 'global' AS grant_scope, NULL AS db_name, NULL AS schema_name, NULL AS object_kind,
       NULL AS object_name, NULL AS column_name, p.privilege AS privilege, 'grant' AS effect,
       p.admin_option AS grantable, NULL AS grantor
FROM user_sys_privs p
WHERE p.username = {{ bind("name") }}
UNION ALL
SELECT p.grantee, 'object', NULL, p.table_schema, 'table', p.table_name, NULL, p.privilege, 'grant', p.grantable, p.grantor
FROM all_tab_privs p
WHERE p.grantee = {{ bind("name") }}
UNION ALL
SELECT p.grantee, 'column', NULL, p.table_schema, 'table', p.table_name, p.column_name, p.privilege, 'grant', p.grantable, p.grantor
FROM all_col_privs p
WHERE p.grantee = {{ bind("name") }}

-- [grants_on]
SELECT p.grantee AS grantee, 'object' AS grant_scope, NULL AS db_name, p.owner AS schema_name, 'table' AS object_kind,
       p.table_name AS object_name, NULL AS column_name, p.privilege AS privilege, 'grant' AS effect,
       p.grantable AS grantable, p.grantor AS grantor
FROM dba_tab_privs p
WHERE p.owner = {{ bind("schema") }} AND p.table_name = {{ bind("table") }}
UNION ALL
SELECT p.grantee, 'column', NULL, p.owner, 'table', p.table_name, p.column_name, p.privilege, 'grant', p.grantable, p.grantor
FROM dba_col_privs p
WHERE p.owner = {{ bind("schema") }} AND p.table_name = {{ bind("table") }}
UNION ALL
SELECT p.grantee, 'global', NULL, NULL, NULL, NULL, NULL, p.privilege, 'grant', p.admin_option, NULL
FROM dba_sys_privs p
WHERE p.privilege IN ('SELECT ANY TABLE', 'READ ANY TABLE', 'INSERT ANY TABLE', 'UPDATE ANY TABLE', 'DELETE ANY TABLE', 'ALTER ANY TABLE', 'DROP ANY TABLE')
UNION ALL
SELECT o.owner, 'object', NULL, o.owner, 'table', o.object_name, NULL, 'OWNER', 'implicit', 'YES', NULL
FROM dba_objects o
WHERE o.owner = {{ bind("schema") }} AND o.object_name = {{ bind("table") }}

-- [grants_on@limited]
SELECT p.grantee AS grantee, 'object' AS grant_scope, NULL AS db_name, p.table_schema AS schema_name, 'table' AS object_kind,
       p.table_name AS object_name, NULL AS column_name, p.privilege AS privilege, 'grant' AS effect,
       p.grantable AS grantable, p.grantor AS grantor
FROM all_tab_privs p
WHERE p.table_schema = {{ bind("schema") }} AND p.table_name = {{ bind("table") }}
UNION ALL
SELECT p.grantee, 'column', NULL, p.table_schema, 'table', p.table_name, p.column_name, p.privilege, 'grant', p.grantable, p.grantor
FROM all_col_privs p
WHERE p.table_schema = {{ bind("schema") }} AND p.table_name = {{ bind("table") }}

-- [owned]
SELECT object_name, object_type AS object_kind
FROM all_objects
WHERE owner = {{ bind("name") }} AND ROWNUM <= 50
