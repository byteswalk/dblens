-- OceanBase (Oracle mode) overrides of the Oracle privilege queries. There is no ORACLE_MAINTAINED column,
-- so built-in accounts and roles are listed by name. DBA_TAB_PRIVS has no TYPE column, and ALL_COL_PRIVS
-- names the owner OWNER where Oracle says TABLE_SCHEMA.

-- [principals]
SELECT u.username AS id, u.username AS name, NULL AS host, 'user' AS principal_kind, 'server' AS principal_level,
       NULL AS can_login,
       CASE WHEN u.account_status LIKE '%LOCKED%' THEN 1 ELSE 0 END AS locked,
       TO_CHAR(u.expiry_date, 'YYYY-MM-DD HH24:MI:SS') AS expires_at,
       CASE WHEN u.username IN ('SYS', 'LBACSYS', 'ORAAUDITOR') THEN 1 ELSE 0 END AS builtin,
       u.account_status AS attr_status,
       u.profile AS attr_profile,
       u.default_tablespace AS attr_default_tablespace
FROM dba_users u
UNION ALL
SELECT r.role, r.role, NULL, 'role', 'server', NULL, NULL, NULL,
       CASE WHEN r.role IN ('CONNECT', 'DBA', 'PUBLIC', 'RESOURCE', 'STANDBY_REPLICATION') THEN 1 ELSE 0 END,
       NULL, NULL, NULL
FROM dba_roles r
ORDER BY 2

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
SELECT p.grantee, 'column', NULL, p.owner, 'table', p.table_name, p.column_name, p.privilege, 'grant', p.grantable, p.grantor
FROM all_col_privs p
WHERE p.grantee = {{ bind("name") }}

-- [grants_on@limited]
SELECT p.grantee AS grantee, 'object' AS grant_scope, NULL AS db_name, p.table_schema AS schema_name, 'table' AS object_kind,
       p.table_name AS object_name, NULL AS column_name, p.privilege AS privilege, 'grant' AS effect,
       p.grantable AS grantable, p.grantor AS grantor
FROM all_tab_privs p
WHERE p.table_schema = {{ bind("schema") }} AND p.table_name = {{ bind("table") }}
UNION ALL
SELECT p.grantee, 'column', NULL, p.owner, 'table', p.table_name, p.column_name, p.privilege, 'grant', p.grantable, p.grantor
FROM all_col_privs p
WHERE p.owner = {{ bind("schema") }} AND p.table_name = {{ bind("table") }}
