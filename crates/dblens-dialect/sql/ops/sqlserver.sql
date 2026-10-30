-- SQL Server ops queries (M6a). Dynamic management views need VIEW SERVER STATE (VIEW SERVER PERFORMANCE STATE on
-- 2022+); without it the database's own permission error is shown.

-- [sessions]
-- name.zh-CN: 会话列表
-- name.en: Sessions
-- category: session
SELECT s.session_id, s.login_name AS [user], s.host_name AS host, DB_NAME(s.database_id) AS db,
       s.program_name AS application, s.status AS state,
       DATEDIFF(SECOND, COALESCE(r.start_time, s.last_request_start_time), SYSDATETIME()) AS seconds,
       LEFT(t.text, 200) AS current_sql,
       'KILL ' + CAST(s.session_id AS varchar(10)) AS kill_sql
FROM sys.dm_exec_sessions s
LEFT JOIN sys.dm_exec_requests r ON r.session_id = s.session_id
OUTER APPLY sys.dm_exec_sql_text(r.sql_handle) t
WHERE s.is_user_process = 1
ORDER BY s.last_request_start_time DESC

-- [running]
-- name.zh-CN: 正在执行的语句
-- name.en: Running Statements
-- category: session
SELECT r.session_id, s.login_name AS [user], s.host_name AS host, DB_NAME(r.database_id) AS db,
       DATEDIFF(SECOND, r.start_time, SYSDATETIME()) AS seconds,
       r.status, r.wait_type, r.blocking_session_id,
       LEFT(t.text, 500) AS current_sql,
       'KILL ' + CAST(r.session_id AS varchar(10)) AS kill_sql
FROM sys.dm_exec_requests r
JOIN sys.dm_exec_sessions s ON s.session_id = r.session_id
OUTER APPLY sys.dm_exec_sql_text(r.sql_handle) t
WHERE s.is_user_process = 1 AND r.session_id <> @@SPID
ORDER BY r.start_time

-- [locks]
-- name.zh-CN: 锁等待
-- name.en: Lock Waits
-- category: lock
SELECT w.session_id AS waiting_session, LEFT(wt.text, 200) AS waiting_sql,
       w.blocking_session_id AS blocking_session, LEFT(bt.text, 200) AS blocking_sql,
       w.wait_type, w.wait_time / 1000 AS wait_seconds, w.wait_resource,
       'KILL ' + CAST(w.blocking_session_id AS varchar(10)) AS kill_sql
FROM sys.dm_exec_requests w
OUTER APPLY sys.dm_exec_sql_text(w.sql_handle) wt
LEFT JOIN sys.dm_exec_connections bc ON bc.session_id = w.blocking_session_id
OUTER APPLY sys.dm_exec_sql_text(bc.most_recent_sql_handle) bt
WHERE w.blocking_session_id <> 0
ORDER BY w.wait_time DESC

-- [storage]
-- name.zh-CN: 各库大小
-- name.en: Database Sizes
-- category: storage
SELECT d.name AS db,
       CAST(SUM(CASE WHEN f.type = 0 THEN f.size END) * 8 / 1024.0 AS decimal(18, 2)) AS data_mb,
       CAST(SUM(CASE WHEN f.type = 1 THEN f.size END) * 8 / 1024.0 AS decimal(18, 2)) AS log_mb,
       CAST(SUM(f.size) * 8 / 1024.0 AS decimal(18, 2)) AS size_mb
FROM sys.databases d
JOIN sys.master_files f ON f.database_id = d.database_id
GROUP BY d.name
ORDER BY SUM(f.size) DESC

-- [largest_tables]
-- name.zh-CN: 占用最大的表（当前库）
-- name.en: Largest Tables (current database)
-- category: storage
SELECT TOP 20 SCHEMA_NAME(t.schema_id) AS [schema], t.name AS table_name,
       SUM(CASE WHEN ps.index_id IN (0, 1) THEN ps.row_count END) AS est_rows,
       CAST(SUM(CASE WHEN ps.index_id IN (0, 1) THEN ps.used_page_count END) * 8 / 1024.0 AS decimal(18, 2)) AS data_mb,
       CAST(SUM(CASE WHEN ps.index_id > 1 THEN ps.used_page_count END) * 8 / 1024.0 AS decimal(18, 2)) AS index_mb,
       CAST(SUM(ps.reserved_page_count) * 8 / 1024.0 AS decimal(18, 2)) AS total_mb
FROM sys.dm_db_partition_stats ps
JOIN sys.tables t ON t.object_id = ps.object_id
WHERE t.is_ms_shipped = 0
GROUP BY t.schema_id, t.name
ORDER BY SUM(ps.reserved_page_count) DESC

-- [slow_sql]
-- name.zh-CN: 慢 SQL
-- name.en: Slow SQL
-- category: sql
SELECT TOP 20 LEFT(t.text, 300) AS sql_text, qs.execution_count AS executions,
       CAST(qs.total_elapsed_time / qs.execution_count / 1000.0 AS decimal(18, 2)) AS avg_ms,
       CAST(qs.max_elapsed_time / 1000.0 AS decimal(18, 2)) AS max_ms,
       CAST(qs.total_elapsed_time / 1000.0 AS decimal(18, 2)) AS total_ms,
       qs.total_rows AS [rows]
FROM sys.dm_exec_query_stats qs
CROSS APPLY sys.dm_exec_sql_text(qs.sql_handle) t
ORDER BY qs.total_elapsed_time / qs.execution_count DESC
