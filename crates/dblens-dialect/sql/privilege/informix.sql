-- GBase 8s（Informix 系）：账号是操作系统用户，sysusers 只记录在这个库里有权限的名字，角色 usertype = 'G'。
-- 数据库级权限是 usertype：D（DBA）、R（RESOURCE）、C（CONNECT）。
-- systabauth.tabauth 共 9 位：s u * i d x a r n，第 3 位表示有列权限，大写表示可转授；syscolauth.colauth 共 3 位：s u r。
-- 系统表的字符列带尾随空格，比较和输出都要 TRIM。

-- [principals]
SELECT TRIM(u.username) AS id,
       TRIM(u.username) AS name,
       CAST(NULL AS VARCHAR(10)) AS host,
       CASE WHEN u.usertype = 'G' THEN 'role' ELSE 'user' END AS principal_kind,
       'database' AS principal_level,
       CASE WHEN u.usertype = 'G' THEN 0 ELSE 1 END AS can_login,
       CAST(NULL AS VARCHAR(10)) AS locked,
       CAST(NULL AS VARCHAR(10)) AS expires_at,
       CASE WHEN TRIM(u.username) IN ('gbasedbt', 'informix', 'public') THEN 1 ELSE 0 END AS builtin,
       TRIM(u.usertype) AS attr_usertype,
       TRIM(u.defrole) AS attr_default_role
FROM sysusers u
ORDER BY 1

-- [memberships]
SELECT TRIM(r.grantee) AS member_id,
       TRIM(r.rolename) AS role_id,
       r.is_grantable AS admin_option,
       CAST(NULL AS VARCHAR(10)) AS inherits,
       CASE WHEN TRIM(u.defrole) = TRIM(r.rolename) THEN 1 ELSE 0 END AS default_role
FROM sysroleauth r, OUTER sysusers u
WHERE u.username = r.grantee

-- [grants_of]
SELECT TRIM(u.username) AS grantee, 'database' AS grant_scope, CAST(NULL AS VARCHAR(128)) AS db_name,
       CAST(NULL AS VARCHAR(128)) AS schema_name, CAST(NULL AS VARCHAR(20)) AS object_kind,
       CAST(NULL AS VARCHAR(128)) AS object_name, CAST(NULL AS VARCHAR(128)) AS column_name,
       CAST(CASE u.usertype WHEN 'D' THEN 'DBA' WHEN 'R' THEN 'RESOURCE' ELSE 'CONNECT' END AS VARCHAR(20)) AS privilege,
       'grant' AS effect, 'NO' AS grantable, CAST(NULL AS VARCHAR(128)) AS grantor
FROM sysusers u
WHERE TRIM(u.username) = {{ bind("name") }} AND u.usertype IN ('D', 'R', 'C')
{% for p in [[1, "SELECT"], [2, "UPDATE"], [4, "INSERT"], [5, "DELETE"], [6, "INDEX"], [7, "ALTER"], [8, "REFERENCES"], [9, "UNDER"]] %}
UNION ALL
SELECT TRIM(a.grantee), 'object', CAST(NULL AS VARCHAR(128)), TRIM(t.owner),
       CAST(CASE t.tabtype WHEN 'V' THEN 'view' ELSE 'table' END AS VARCHAR(20)), TRIM(t.tabname),
       CAST(NULL AS VARCHAR(128)), CAST('{{ p[1] }}' AS VARCHAR(20)), 'grant',
       CASE WHEN SUBSTR(a.tabauth, {{ p[0] }}, 1) = UPPER(SUBSTR(a.tabauth, {{ p[0] }}, 1)) THEN 'YES' ELSE 'NO' END,
       TRIM(a.grantor)
FROM systabauth a, systables t
WHERE t.tabid = a.tabid AND t.tabid >= 100 AND t.tabtype IN ('T', 'V', 'S', 'E')
  AND TRIM(a.grantee) = {{ bind("name") }} AND SUBSTR(a.tabauth, {{ p[0] }}, 1) <> '-'
{% endfor %}
{% for p in [[1, "SELECT"], [2, "UPDATE"], [3, "REFERENCES"]] %}
UNION ALL
SELECT TRIM(a.grantee), 'column', CAST(NULL AS VARCHAR(128)), TRIM(t.owner), CAST('table' AS VARCHAR(20)),
       TRIM(t.tabname), TRIM(c.colname), CAST('{{ p[1] }}' AS VARCHAR(20)), 'grant',
       CASE WHEN SUBSTR(a.colauth, {{ p[0] }}, 1) = UPPER(SUBSTR(a.colauth, {{ p[0] }}, 1)) THEN 'YES' ELSE 'NO' END,
       TRIM(a.grantor)
FROM syscolauth a, systables t, syscolumns c
WHERE t.tabid = a.tabid AND c.tabid = a.tabid AND c.colno = a.colno AND t.tabid >= 100
  AND TRIM(a.grantee) = {{ bind("name") }} AND SUBSTR(a.colauth, {{ p[0] }}, 1) <> '-'
{% endfor %}
UNION ALL
SELECT TRIM(a.grantee), 'object', CAST(NULL AS VARCHAR(128)), TRIM(p.owner),
       CAST(CASE p.isproc WHEN 't' THEN 'procedure' ELSE 'function' END AS VARCHAR(20)), TRIM(p.procname),
       CAST(NULL AS VARCHAR(128)), CAST('EXECUTE' AS VARCHAR(20)), 'grant',
       CASE WHEN a.procauth = 'E' THEN 'YES' ELSE 'NO' END, TRIM(a.grantor)
FROM sysprocauth a, sysprocedures p
WHERE p.procid = a.procid AND p.internal = 'f' AND TRIM(p.owner) NOT IN ('informix', 'gbasedbt')
  AND TRIM(a.grantee) = {{ bind("name") }}

-- [grants_on]
SELECT TRIM(u.username) AS grantee, 'database' AS grant_scope, CAST(NULL AS VARCHAR(128)) AS db_name,
       CAST(NULL AS VARCHAR(128)) AS schema_name, CAST(NULL AS VARCHAR(20)) AS object_kind,
       CAST(NULL AS VARCHAR(128)) AS object_name, CAST(NULL AS VARCHAR(128)) AS column_name,
       CAST('DBA' AS VARCHAR(20)) AS privilege, 'implicit' AS effect, 'NO' AS grantable,
       CAST(NULL AS VARCHAR(128)) AS grantor
FROM sysusers u
WHERE u.usertype = 'D'
UNION ALL
SELECT TRIM(t.owner), 'object', CAST(NULL AS VARCHAR(128)), TRIM(t.owner), CAST('table' AS VARCHAR(20)),
       TRIM(t.tabname), CAST(NULL AS VARCHAR(128)), CAST('OWNER' AS VARCHAR(20)), 'implicit', 'NO',
       CAST(NULL AS VARCHAR(128))
FROM systables t
WHERE TRIM(t.owner) = {{ bind("schema") }} AND TRIM(t.tabname) = {{ bind("table") }}
{% for p in [[1, "SELECT"], [2, "UPDATE"], [4, "INSERT"], [5, "DELETE"], [6, "INDEX"], [7, "ALTER"], [8, "REFERENCES"], [9, "UNDER"]] %}
UNION ALL
SELECT TRIM(a.grantee), 'object', CAST(NULL AS VARCHAR(128)), TRIM(t.owner),
       CAST(CASE t.tabtype WHEN 'V' THEN 'view' ELSE 'table' END AS VARCHAR(20)), TRIM(t.tabname),
       CAST(NULL AS VARCHAR(128)), CAST('{{ p[1] }}' AS VARCHAR(20)), 'grant',
       CASE WHEN SUBSTR(a.tabauth, {{ p[0] }}, 1) = UPPER(SUBSTR(a.tabauth, {{ p[0] }}, 1)) THEN 'YES' ELSE 'NO' END,
       TRIM(a.grantor)
FROM systabauth a, systables t
WHERE t.tabid = a.tabid AND TRIM(t.owner) = {{ bind("schema") }} AND TRIM(t.tabname) = {{ bind("table") }}
  AND SUBSTR(a.tabauth, {{ p[0] }}, 1) <> '-'
{% endfor %}
{% for p in [[1, "SELECT"], [2, "UPDATE"], [3, "REFERENCES"]] %}
UNION ALL
SELECT TRIM(a.grantee), 'column', CAST(NULL AS VARCHAR(128)), TRIM(t.owner), CAST('table' AS VARCHAR(20)),
       TRIM(t.tabname), TRIM(c.colname), CAST('{{ p[1] }}' AS VARCHAR(20)), 'grant',
       CASE WHEN SUBSTR(a.colauth, {{ p[0] }}, 1) = UPPER(SUBSTR(a.colauth, {{ p[0] }}, 1)) THEN 'YES' ELSE 'NO' END,
       TRIM(a.grantor)
FROM syscolauth a, systables t, syscolumns c
WHERE t.tabid = a.tabid AND c.tabid = a.tabid AND c.colno = a.colno
  AND TRIM(t.owner) = {{ bind("schema") }} AND TRIM(t.tabname) = {{ bind("table") }}
  AND SUBSTR(a.colauth, {{ p[0] }}, 1) <> '-'
{% endfor %}
