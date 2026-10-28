-- OceanBase (MySQL mode) overrides of the MySQL ops queries. There is neither the sys schema nor
-- performance_schema statement digests: locks come from GV$OB_LOCKS (a waiting row has BLOCK = 1, ID1 is the
-- transaction it waits for) and slow SQL from the plan cache statistics. Times there are microseconds.

-- [locks]
-- name.zh-CN: 锁等待
-- name.en: Lock Waits
-- category: lock
SELECT w.SESSION_ID AS waiting_pid, LEFT(wp.INFO, 200) AS waiting_sql,
       h.SESSION_ID AS blocking_pid, LEFT(hp.INFO, 200) AS blocking_sql,
       w.TYPE AS lock_type, w.CTIME AS wait_seconds,
       CONCAT('KILL ', h.SESSION_ID) AS kill_sql
FROM oceanbase.GV$OB_LOCKS w
LEFT JOIN oceanbase.GV$OB_LOCKS h ON h.TRANS_ID = w.ID1 AND h.BLOCK = 0
LEFT JOIN oceanbase.GV$OB_PROCESSLIST wp ON wp.ID = w.SESSION_ID
LEFT JOIN oceanbase.GV$OB_PROCESSLIST hp ON hp.ID = h.SESSION_ID
WHERE w.BLOCK = 1
ORDER BY w.CTIME DESC

-- [slow_sql]
-- name.zh-CN: 慢 SQL
-- name.en: Slow SQL
-- category: sql
SELECT LEFT(p.QUERY_SQL, 300) AS sql_text, p.EXECUTIONS AS executions,
       ROUND(p.AVG_EXE_USEC / 1000, 2) AS avg_ms,
       ROUND(p.SLOWEST_EXE_USEC / 1000, 2) AS max_ms,
       ROUND(p.ELAPSED_TIME / 1000, 2) AS total_ms,
       p.ROWS_PROCESSED AS rows_processed, p.LAST_ACTIVE_TIME AS last_seen
FROM oceanbase.GV$OB_PLAN_CACHE_PLAN_STAT p
ORDER BY p.AVG_EXE_USEC DESC
LIMIT 20
