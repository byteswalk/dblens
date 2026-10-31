-- 达梦。规划时没有实连（系统账号密码未保存），Task 5 第一步实测后修正。结构同 Oracle，
-- 但 ALL_TAB_PRIVS 的模式列叫 OWNER，没有 oracle_maintained，内置账号和角色按名字判断。

-- [principals]
SELECT u.username AS id, u.username AS name, NULL AS host, 'user' AS principal_kind, 'server' AS principal_level,
       NULL AS can_login,
       CASE WHEN u.account_status LIKE '%LOCK%' THEN 1 ELSE 0 END AS locked,
       TO_CHAR(u.expiry_date, 'YYYY-MM-DD HH24:MI:SS') AS expires_at,
       CASE WHEN u.username IN ('SYS', 'SYSDBA', 'SYSAUDITOR', 'SYSSSO', 'SYSDBO') THEN 1 ELSE 0 END AS builtin,
       u.account_status AS attr_status
FROM dba_users u
UNION ALL
SELECT r.role, r.role, NULL, 'role', 'server', NULL, NULL, NULL,
       CASE WHEN r.role IN ('DBA', 'RESOURCE', 'PUBLIC', 'VTI', 'SOI') OR r.role LIKE 'DB!_%' ESCAPE '!' THEN 1 ELSE 0 END,
       NULL
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

-- [grants_of]
SELECT p.grantee AS grantee, 'global' AS grant_scope, NULL AS db_name, NULL AS schema_name, NULL AS object_kind,
       NULL AS object_name, NULL AS column_name, p.privilege AS privilege, 'grant' AS effect,
       p.admin_option AS grantable, NULL AS grantor
FROM dba_sys_privs p
WHERE p.grantee = {{ bind("name") }}
UNION ALL
SELECT p.grantee, 'object', NULL, p.owner, 'table', p.table_name, NULL, p.privilege, 'grant', p.grantable, p.grantor
FROM dba_tab_privs p
WHERE p.grantee = {{ bind("name") }}
UNION ALL
SELECT p.grantee, 'column', NULL, p.owner, 'table', p.table_name, p.column_name, p.privilege, 'grant', p.grantable, p.grantor
FROM dba_col_privs p
WHERE p.grantee = {{ bind("name") }}

-- [grants_of@limited]
SELECT p.grantee AS grantee, 'object' AS grant_scope, NULL AS db_name, p.owner AS schema_name, 'table' AS object_kind,
       p.table_name AS object_name, NULL AS column_name, p.privilege AS privilege, 'grant' AS effect,
       p.grantable AS grantable, p.grantor AS grantor
FROM all_tab_privs p
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
WHERE p.privilege IN ('SELECT ANY TABLE', 'INSERT ANY TABLE', 'UPDATE ANY TABLE', 'DELETE ANY TABLE', 'ALTER ANY TABLE', 'DROP ANY TABLE')
UNION ALL
SELECT o.owner, 'object', NULL, o.owner, 'table', o.object_name, NULL, 'OWNER', 'implicit', 'YES', NULL
FROM dba_objects o
WHERE o.owner = {{ bind("schema") }} AND o.object_name = {{ bind("table") }}

-- [grants_on@limited]
SELECT p.grantee AS grantee, 'object' AS grant_scope, NULL AS db_name, p.owner AS schema_name, 'table' AS object_kind,
       p.table_name AS object_name, NULL AS column_name, p.privilege AS privilege, 'grant' AS effect,
       p.grantable AS grantable, p.grantor AS grantor
FROM all_tab_privs p
WHERE p.owner = {{ bind("schema") }} AND p.table_name = {{ bind("table") }}

-- [owned]
SELECT object_name, object_type AS object_kind
FROM all_objects
WHERE owner = {{ bind("name") }} AND ROWNUM <= 50
