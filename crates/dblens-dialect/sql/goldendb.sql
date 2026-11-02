-- GoldenDB overrides. information_schema.TRIGGERS has no user triggers here, only SHOW TRIGGERS lists them.
-- SHOW takes no bound schema; Dialect::adapt_rows turns its columns into the ones `triggers` must return.

-- [schemas]
SELECT s.SCHEMA_NAME AS name,
       CASE WHEN LOWER(s.SCHEMA_NAME) IN ('mysql', 'sys', 'information_schema', 'performance_schema', 'dbagent', 'heartbeat_info', 'recyclebin')
              OR LOWER(s.SCHEMA_NAME) LIKE '!_gdb!_%' ESCAPE '!' THEN 1 ELSE 0 END AS is_system
FROM information_schema.SCHEMATA s
ORDER BY s.SCHEMA_NAME

-- [triggers]
SHOW TRIGGERS FROM {{ schema_ident }}
