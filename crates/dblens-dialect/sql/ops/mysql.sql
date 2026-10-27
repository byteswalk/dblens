-- MySQL ops queries (M1c3). Also used by TiDB, OceanBase and PolarDB, which may lack some of the views:
-- the database's own error is shown in that case. Timers in performance_schema are picoseconds.

-- [sessions]
-- name.zh-CN: 会话列表
-- name.en: Sessions
-- category: session
SELECT p.ID AS id, p.USER AS user, p.HOST AS host, p.DB AS db, p.COMMAND AS command,
       p.TIME AS seconds, p.STATE AS state, LEFT(p.INFO, 200) AS current_sql,
       CONCAT('KILL ', p.ID) AS kill_sql
FROM information_schema.PROCESSLIST p
ORDER BY p.TIME DESC

-- [running]
-- name.zh-CN: 正在执行的语句
-- name.en: Running Statements
-- category: session
SELECT p.ID AS id, p.USER AS user, p.HOST AS host, p.DB AS db, p.TIME AS seconds, p.STATE AS state,
       LEFT(p.INFO, 500) AS current_sql, CONCAT('KILL ', p.ID) AS kill_sql
FROM information_schema.PROCESSLIST p
WHERE p.COMMAND NOT IN ('Sleep', 'Daemon', 'Binlog Dump') AND p.ID <> CONNECTION_ID()
ORDER BY p.TIME DESC

-- [locks]
-- name.zh-CN: 锁等待
-- name.en: Lock Waits
-- category: lock
SELECT w.waiting_pid, LEFT(w.waiting_query, 200) AS waiting_sql,
       w.blocking_pid, LEFT(w.blocking_query, 200) AS blocking_sql,
       w.locked_table, w.wait_age_secs AS wait_seconds,
       CONCAT('KILL ', w.blocking_pid) AS kill_sql
FROM sys.innodb_lock_waits w
ORDER BY w.wait_age_secs DESC

-- [storage]
-- name.zh-CN: 各库大小
-- name.en: Database Sizes
-- category: storage
SELECT t.TABLE_SCHEMA AS db, COUNT(*) AS tables,
       ROUND(SUM(t.DATA_LENGTH) / 1048576, 2) AS data_mb,
       ROUND(SUM(t.INDEX_LENGTH) / 1048576, 2) AS index_mb,
       ROUND(SUM(t.DATA_LENGTH + t.INDEX_LENGTH) / 1048576, 2) AS total_mb
FROM information_schema.TABLES t
WHERE t.TABLE_TYPE = 'BASE TABLE'
GROUP BY t.TABLE_SCHEMA
ORDER BY total_mb DESC

-- [largest_tables]
-- name.zh-CN: 占用最大的表
-- name.en: Largest Tables
-- category: storage
SELECT t.TABLE_SCHEMA AS db, t.TABLE_NAME AS table_name, t.TABLE_ROWS AS est_rows,
       ROUND(t.DATA_LENGTH / 1048576, 2) AS data_mb,
       ROUND(t.INDEX_LENGTH / 1048576, 2) AS index_mb,
       ROUND((t.DATA_LENGTH + t.INDEX_LENGTH) / 1048576, 2) AS total_mb
FROM information_schema.TABLES t
WHERE t.TABLE_TYPE = 'BASE TABLE'
ORDER BY t.DATA_LENGTH + t.INDEX_LENGTH DESC
LIMIT 20

-- [slow_sql]
-- name.zh-CN: 慢 SQL
-- name.en: Slow SQL
-- category: sql
SELECT d.SCHEMA_NAME AS db, LEFT(d.DIGEST_TEXT, 300) AS sql_text, d.COUNT_STAR AS executions,
       ROUND(d.AVG_TIMER_WAIT / 1000000000, 2) AS avg_ms,
       ROUND(d.MAX_TIMER_WAIT / 1000000000, 2) AS max_ms,
       ROUND(d.SUM_TIMER_WAIT / 1000000000, 2) AS total_ms,
       d.SUM_ROWS_EXAMINED AS rows_examined, d.LAST_SEEN AS last_seen
FROM performance_schema.events_statements_summary_by_digest d
WHERE d.DIGEST_TEXT IS NOT NULL
ORDER BY d.AVG_TIMER_WAIT DESC
LIMIT 20
