-- openGauss overrides of the PostgreSQL ops queries. pg_stat_activity has no backend_type / wait_event_type
-- (it has waiting / enqueue), there is no pg_blocking_pids, and statement statistics are in dbe_perf.statement
-- (microseconds) instead of pg_stat_statements.

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
WHERE a.usename IS NOT NULL
ORDER BY a.query_start NULLS LAST

-- [running]
-- name.zh-CN: 正在执行的语句
-- name.en: Running Statements
-- category: session
SELECT a.pid, a.usename AS user, a.client_addr::text AS host, a.datname AS db,
       ROUND(EXTRACT(EPOCH FROM now() - a.query_start)) AS seconds,
       a.waiting, a.enqueue, LEFT(a.query, 500) AS current_sql,
       'SELECT pg_terminate_backend(' || a.pid || ')' AS kill_sql
FROM pg_stat_activity a
WHERE a.usename IS NOT NULL AND a.state = 'active' AND a.pid <> pg_backend_pid()
ORDER BY a.query_start

-- [locks]
-- name.zh-CN: 锁等待
-- name.en: Lock Waits
-- category: lock
SELECT w.pid AS waiting_pid, LEFT(wa.query, 200) AS waiting_sql,
       h.pid AS blocking_pid, LEFT(ha.query, 200) AS blocking_sql,
       w.locktype AS lock_type, w.relation::regclass::text AS locked_object,
       ROUND(EXTRACT(EPOCH FROM now() - wa.query_start)) AS wait_seconds,
       'SELECT pg_terminate_backend(' || h.pid || ')' AS kill_sql
FROM pg_locks w
JOIN pg_locks h ON h.granted AND NOT w.granted AND h.pid <> w.pid AND h.locktype = w.locktype
               AND h.database IS NOT DISTINCT FROM w.database
               AND h.relation IS NOT DISTINCT FROM w.relation
               AND h.transactionid IS NOT DISTINCT FROM w.transactionid
JOIN pg_stat_activity wa ON wa.pid = w.pid
JOIN pg_stat_activity ha ON ha.pid = h.pid
ORDER BY wait_seconds DESC

-- [slow_sql]
-- name.zh-CN: 慢 SQL
-- name.en: Slow SQL
-- category: sql
SELECT s.user_name AS user, LEFT(s.query, 300) AS sql_text, s.n_calls AS executions,
       ROUND(s.total_elapse_time / s.n_calls / 1000.0, 2) AS avg_ms,
       ROUND(s.max_elapse_time / 1000.0, 2) AS max_ms,
       ROUND(s.total_elapse_time / 1000.0, 2) AS total_ms,
       s.n_returned_rows AS rows_returned, s.last_updated AS last_seen
FROM dbe_perf.statement s
WHERE s.n_calls > 0
ORDER BY s.total_elapse_time / s.n_calls DESC
LIMIT 20
