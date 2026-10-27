-- PostgreSQL ops queries (M1c3). Also used by KingbaseES, openGauss, GaussDB and HighGo, which may name
-- some columns differently: the database's own error is shown in that case.
-- slow_sql needs the pg_stat_statements extension (PostgreSQL 13+ column names).

-- [sessions]
-- name.zh-CN: 会话列表
-- name.en: Sessions
-- category: session
SELECT a.pid, a.usename AS user, a.client_addr::text AS host, a.datname AS db,
       a.application_name AS application, a.state,
       ROUND(EXTRACT(EPOCH FROM now() - a.query_start)) AS seconds,
       LEFT(a.query, 200) AS current_sql,
       'SELECT pg_terminate_backend(' || a.pid || ')' AS kill_sql
FROM pg_stat_activity a
WHERE a.backend_type = 'client backend'
ORDER BY a.query_start NULLS LAST

-- [running]
-- name.zh-CN: 正在执行的语句
-- name.en: Running Statements
-- category: session
SELECT a.pid, a.usename AS user, a.client_addr::text AS host, a.datname AS db,
       ROUND(EXTRACT(EPOCH FROM now() - a.query_start)) AS seconds,
       a.wait_event_type, a.wait_event, LEFT(a.query, 500) AS current_sql,
       'SELECT pg_terminate_backend(' || a.pid || ')' AS kill_sql
FROM pg_stat_activity a
WHERE a.backend_type = 'client backend' AND a.state = 'active' AND a.pid <> pg_backend_pid()
ORDER BY a.query_start

-- [locks]
-- name.zh-CN: 锁等待
-- name.en: Lock Waits
-- category: lock
SELECT w.pid AS waiting_pid, LEFT(w.query, 200) AS waiting_sql,
       b.pid AS blocking_pid, LEFT(b.query, 200) AS blocking_sql,
       ROUND(EXTRACT(EPOCH FROM now() - w.query_start)) AS wait_seconds,
       'SELECT pg_terminate_backend(' || b.pid || ')' AS kill_sql
FROM pg_stat_activity w
CROSS JOIN LATERAL unnest(pg_blocking_pids(w.pid)) AS bp(pid)
JOIN pg_stat_activity b ON b.pid = bp.pid
ORDER BY wait_seconds DESC

-- [storage]
-- name.zh-CN: 各库大小
-- name.en: Database Sizes
-- category: storage
SELECT d.datname AS db, pg_size_pretty(pg_database_size(d.datname)) AS size,
       ROUND(pg_database_size(d.datname) / 1048576.0, 2) AS size_mb
FROM pg_database d
WHERE NOT d.datistemplate AND has_database_privilege(d.datname, 'CONNECT')
ORDER BY pg_database_size(d.datname) DESC

-- [largest_tables]
-- name.zh-CN: 占用最大的表
-- name.en: Largest Tables
-- category: storage
SELECT n.nspname AS schema, c.relname AS table_name, c.reltuples::bigint AS est_rows,
       ROUND(pg_table_size(c.oid) / 1048576.0, 2) AS data_mb,
       ROUND(pg_indexes_size(c.oid) / 1048576.0, 2) AS index_mb,
       ROUND(pg_total_relation_size(c.oid) / 1048576.0, 2) AS total_mb
FROM pg_class c
JOIN pg_namespace n ON n.oid = c.relnamespace
WHERE c.relkind IN ('r', 'p', 'm')
  AND n.nspname NOT IN ('pg_catalog', 'information_schema') AND n.nspname NOT LIKE 'pg_toast%'
ORDER BY pg_total_relation_size(c.oid) DESC
LIMIT 20

-- [slow_sql]
-- name.zh-CN: 慢 SQL
-- name.en: Slow SQL
-- category: sql
SELECT LEFT(s.query, 300) AS sql_text, s.calls AS executions,
       ROUND(s.mean_exec_time::numeric, 2) AS avg_ms,
       ROUND(s.max_exec_time::numeric, 2) AS max_ms,
       ROUND(s.total_exec_time::numeric, 2) AS total_ms,
       s.rows
FROM pg_stat_statements s
ORDER BY s.mean_exec_time DESC
LIMIT 20
