-- MariaDB：角色在 mysql.user 里 is_role = 'Y'，host 为空；GRANTEE 对角色只写 'role'。

-- [principals]
SELECT CASE WHEN u.is_role = 'Y' THEN CONCAT('''', u.User, '''') ELSE CONCAT('''', u.User, '''@''', u.Host, '''') END AS id,
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
SELECT CASE WHEN m.Host = '' THEN CONCAT('''', m.User, '''') ELSE CONCAT('''', m.User, '''@''', m.Host, '''') END AS member_id,
       CONCAT('''', m.Role, '''') AS role_id,
       m.Admin_option AS admin_option,
       NULL AS inherits,
       CASE WHEN u.default_role = m.Role THEN 1 ELSE 0 END AS default_role
FROM mysql.roles_mapping m
LEFT JOIN mysql.user u ON u.User = m.User AND u.Host = m.Host
