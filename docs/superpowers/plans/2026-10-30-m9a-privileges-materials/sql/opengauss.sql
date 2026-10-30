-- openGauss：aclexplode 不能用 LATERAL，放进子查询；pg_roles 另有 rolsystemadmin 等列。

-- [caps]
-- 普通用户查 pg_roles 不报错，只是看不到别人。
SELECT CASE WHEN r.rolsuper OR r.rolsystemadmin THEN 1 ELSE 0 END AS full_view
FROM pg_catalog.pg_roles r
WHERE r.rolname = current_user

-- [principals]
SELECT r.rolname AS id,
       r.rolname AS name,
       NULL AS host,
       CASE WHEN r.rolcanlogin THEN 'user' ELSE 'role' END AS principal_kind,
       'server' AS principal_level,
       r.rolcanlogin AS can_login,
       NULL AS locked,
       CAST(r.rolvaliduntil AS varchar(40)) AS expires_at,
       CASE WHEN r.oid < 16384 THEN 1 ELSE 0 END AS builtin,
       r.rolsuper AS attr_superuser,
       r.rolsystemadmin AS attr_sysadmin,
       r.rolcreatedb AS attr_createdb,
       r.rolcreaterole AS attr_createrole,
       r.rolinherit AS attr_inherit
FROM pg_catalog.pg_roles r
ORDER BY r.rolname

-- [grants_of]
SELECT g.grantee, g.grant_scope, g.db_name, g.schema_name, g.object_kind, g.object_name, g.column_name,
       g.privilege, 'grant' AS effect, g.grantable, g.grantor
FROM (
  SELECT CASE WHEN (x.acl).grantee = 0 THEN 'PUBLIC' ELSE pg_catalog.pg_get_userbyid((x.acl).grantee) END AS grantee,
         'object' AS grant_scope, NULL AS db_name, n.nspname AS schema_name,
         CASE x.relkind WHEN 'v' THEN 'view' WHEN 'm' THEN 'materialized_view' WHEN 'S' THEN 'sequence' ELSE 'table' END AS object_kind,
         x.relname AS object_name, NULL AS column_name, (x.acl).privilege_type AS privilege, (x.acl).is_grantable AS grantable,
         pg_catalog.pg_get_userbyid((x.acl).grantor) AS grantor
  FROM (SELECT c.relname, c.relkind, c.relnamespace, pg_catalog.aclexplode(c.relacl) AS acl
        FROM pg_catalog.pg_class c WHERE c.relkind IN ('r', 'v', 'm', 'f', 'S')) x
  JOIN pg_catalog.pg_namespace n ON n.oid = x.relnamespace
  WHERE n.nspname NOT IN ('pg_catalog', 'information_schema') AND n.nspname NOT LIKE 'pg!_%' ESCAPE '!' AND n.nspname NOT LIKE 'dbe!_%' ESCAPE '!'
  UNION ALL
  SELECT CASE WHEN (x.acl).grantee = 0 THEN 'PUBLIC' ELSE pg_catalog.pg_get_userbyid((x.acl).grantee) END,
         'schema', NULL, x.nspname, NULL, NULL, NULL, (x.acl).privilege_type, (x.acl).is_grantable,
         pg_catalog.pg_get_userbyid((x.acl).grantor)
  FROM (SELECT n.nspname, pg_catalog.aclexplode(n.nspacl) AS acl FROM pg_catalog.pg_namespace n) x
  WHERE x.nspname NOT IN ('pg_catalog', 'information_schema') AND x.nspname NOT LIKE 'pg!_%' ESCAPE '!'
  UNION ALL
  SELECT CASE WHEN (x.acl).grantee = 0 THEN 'PUBLIC' ELSE pg_catalog.pg_get_userbyid((x.acl).grantee) END,
         'database', x.datname, NULL, NULL, NULL, NULL, (x.acl).privilege_type, (x.acl).is_grantable,
         pg_catalog.pg_get_userbyid((x.acl).grantor)
  FROM (SELECT d.datname, pg_catalog.aclexplode(d.datacl) AS acl FROM pg_catalog.pg_database d WHERE d.datname = pg_catalog.current_database()) x
) g
WHERE g.grantee = {{ bind("name") }}

-- [grants_on]
SELECT g.grantee, g.grant_scope, NULL AS db_name, g.schema_name, g.object_kind, g.object_name, g.column_name,
       g.privilege, g.effect, g.grantable, g.grantor
FROM (
  SELECT CASE WHEN (x.acl).grantee = 0 THEN 'PUBLIC' ELSE pg_catalog.pg_get_userbyid((x.acl).grantee) END AS grantee,
         'object' AS grant_scope, n.nspname AS schema_name, 'table' AS object_kind, x.relname AS object_name,
         NULL AS column_name, (x.acl).privilege_type AS privilege, 'grant' AS effect, (x.acl).is_grantable AS grantable,
         pg_catalog.pg_get_userbyid((x.acl).grantor) AS grantor
  FROM (SELECT c.relname, c.relnamespace, pg_catalog.aclexplode(c.relacl) AS acl FROM pg_catalog.pg_class c) x
  JOIN pg_catalog.pg_namespace n ON n.oid = x.relnamespace
  WHERE n.nspname = {{ bind("schema") }} AND x.relname = {{ bind("table") }}
  UNION ALL
  SELECT pg_catalog.pg_get_userbyid(c.relowner), 'object', n.nspname, 'table', c.relname, NULL, 'OWNER', 'implicit', TRUE, NULL
  FROM pg_catalog.pg_class c
  JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace
  WHERE n.nspname = {{ bind("schema") }} AND c.relname = {{ bind("table") }}
  UNION ALL
  SELECT r.rolname, 'global', NULL, NULL, NULL, NULL, 'SYSADMIN', 'implicit', TRUE, NULL
  FROM pg_catalog.pg_roles r
  WHERE r.rolsuper OR r.rolsystemadmin
) g
