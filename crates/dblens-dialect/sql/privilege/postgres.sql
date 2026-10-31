-- PG 系。主体 id 就是角色名；PUBLIC 是虚拟主体，grantee 0 显示为 PUBLIC。只看当前库，系统模式不列。

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
       r.rolcreatedb AS attr_createdb,
       r.rolcreaterole AS attr_createrole,
       r.rolreplication AS attr_replication,
       r.rolinherit AS attr_inherit
FROM pg_catalog.pg_roles r
ORDER BY r.rolname

-- [memberships]
SELECT DISTINCT pg_catalog.pg_get_userbyid(m.member) AS member_id,
       pg_catalog.pg_get_userbyid(m.roleid) AS role_id,
       m.admin_option AS admin_option,
{% if version >= 160000 %}
       m.inherit_option AS inherits,
{% else %}
       u.rolinherit AS inherits,
{% endif %}
       NULL AS default_role
FROM pg_catalog.pg_auth_members m
JOIN pg_catalog.pg_roles u ON u.oid = m.member

-- [grants_of]
SELECT g.grantee, g.grant_scope, g.db_name, g.schema_name, g.object_kind, g.object_name, g.column_name,
       g.privilege, 'grant' AS effect, g.grantable, g.grantor
FROM (
  SELECT CASE WHEN a.grantee = 0 THEN 'PUBLIC' ELSE pg_catalog.pg_get_userbyid(a.grantee) END AS grantee,
         'object' AS grant_scope, NULL AS db_name, n.nspname AS schema_name,
         CASE c.relkind WHEN 'v' THEN 'view' WHEN 'm' THEN 'materialized_view' WHEN 'S' THEN 'sequence' ELSE 'table' END AS object_kind,
         c.relname AS object_name, NULL AS column_name, a.privilege_type AS privilege, a.is_grantable AS grantable,
         pg_catalog.pg_get_userbyid(a.grantor) AS grantor
  FROM pg_catalog.pg_class c
  JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace
  CROSS JOIN LATERAL pg_catalog.aclexplode(c.relacl) a
  WHERE c.relkind IN ('r', 'p', 'v', 'm', 'f', 'S')
    AND n.nspname NOT IN ('pg_catalog', 'information_schema') AND n.nspname NOT LIKE 'pg!_toast%' ESCAPE '!'
  UNION ALL
  SELECT CASE WHEN a.grantee = 0 THEN 'PUBLIC' ELSE pg_catalog.pg_get_userbyid(a.grantee) END,
         'column', NULL, n.nspname, 'table', c.relname, t.attname, a.privilege_type, a.is_grantable,
         pg_catalog.pg_get_userbyid(a.grantor)
  FROM pg_catalog.pg_attribute t
  JOIN pg_catalog.pg_class c ON c.oid = t.attrelid
  JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace
  CROSS JOIN LATERAL pg_catalog.aclexplode(t.attacl) a
  WHERE t.attnum > 0 AND NOT t.attisdropped AND t.attacl IS NOT NULL
    AND n.nspname NOT IN ('pg_catalog', 'information_schema')
  UNION ALL
  SELECT CASE WHEN a.grantee = 0 THEN 'PUBLIC' ELSE pg_catalog.pg_get_userbyid(a.grantee) END,
         'schema', NULL, n.nspname, NULL, NULL, NULL, a.privilege_type, a.is_grantable,
         pg_catalog.pg_get_userbyid(a.grantor)
  FROM pg_catalog.pg_namespace n
  CROSS JOIN LATERAL pg_catalog.aclexplode(n.nspacl) a
  WHERE n.nspname NOT IN ('pg_catalog', 'information_schema') AND n.nspname NOT LIKE 'pg!_toast%' ESCAPE '!'
  UNION ALL
  SELECT CASE WHEN a.grantee = 0 THEN 'PUBLIC' ELSE pg_catalog.pg_get_userbyid(a.grantee) END,
         'database', d.datname, NULL, NULL, NULL, NULL, a.privilege_type, a.is_grantable,
         pg_catalog.pg_get_userbyid(a.grantor)
  FROM pg_catalog.pg_database d
  CROSS JOIN LATERAL pg_catalog.aclexplode(d.datacl) a
  WHERE d.datname = pg_catalog.current_database()
) g
WHERE g.grantee = {{ bind("name") }}

-- [grants_on]
SELECT g.grantee, g.grant_scope, NULL AS db_name, g.schema_name, g.object_kind, g.object_name, g.column_name,
       g.privilege, g.effect, g.grantable, g.grantor
FROM (
  SELECT CASE WHEN a.grantee = 0 THEN 'PUBLIC' ELSE pg_catalog.pg_get_userbyid(a.grantee) END AS grantee,
         'object' AS grant_scope, n.nspname AS schema_name, 'table' AS object_kind, c.relname AS object_name,
         NULL AS column_name, a.privilege_type AS privilege, 'grant' AS effect, a.is_grantable AS grantable,
         pg_catalog.pg_get_userbyid(a.grantor) AS grantor
  FROM pg_catalog.pg_class c
  JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace
  CROSS JOIN LATERAL pg_catalog.aclexplode(c.relacl) a
  WHERE n.nspname = {{ bind("schema") }} AND c.relname = {{ bind("table") }}
  UNION ALL
  SELECT CASE WHEN a.grantee = 0 THEN 'PUBLIC' ELSE pg_catalog.pg_get_userbyid(a.grantee) END,
         'column', n.nspname, 'table', c.relname, t.attname, a.privilege_type, 'grant', a.is_grantable,
         pg_catalog.pg_get_userbyid(a.grantor)
  FROM pg_catalog.pg_attribute t
  JOIN pg_catalog.pg_class c ON c.oid = t.attrelid
  JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace
  CROSS JOIN LATERAL pg_catalog.aclexplode(t.attacl) a
  WHERE n.nspname = {{ bind("schema") }} AND c.relname = {{ bind("table") }}
    AND t.attnum > 0 AND NOT t.attisdropped AND t.attacl IS NOT NULL
  UNION ALL
  SELECT pg_catalog.pg_get_userbyid(c.relowner), 'object', n.nspname, 'table', c.relname, NULL, 'OWNER', 'implicit', TRUE, NULL
  FROM pg_catalog.pg_class c
  JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace
  WHERE n.nspname = {{ bind("schema") }} AND c.relname = {{ bind("table") }}
  UNION ALL
  SELECT r.rolname, 'global', NULL, NULL, NULL, NULL, 'SUPERUSER', 'implicit', TRUE, NULL
  FROM pg_catalog.pg_roles r
  WHERE r.rolsuper
) g

-- [owned]
SELECT n.nspname || '.' || c.relname AS object_name, c.relkind::text AS object_kind
FROM pg_catalog.pg_class c
JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace
JOIN pg_catalog.pg_roles r ON r.oid = c.relowner
WHERE r.rolname = {{ bind("name") }} AND c.relkind IN ('r', 'v', 'm', 'S', 'p')
ORDER BY 1
LIMIT 50
