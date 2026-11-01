-- OceanBase (Oracle mode) overrides of the Oracle ops queries. V$SESSION, V$SQL, DBA_DATA_FILES and
-- DBA_FREE_SPACE do not exist: sessions come from V$OB_PROCESSLIST, locks from V$OB_LOCKS (a waiting row has
-- BLOCK = 1 and ID1 is the transaction it waits for), storage from V$OB_UNITS. Times there are microseconds.

-- [sessions]
-- name.zh-CN: 会话列表
-- name.en: Sessions
-- category: session
SELECT p.ID AS "sid", p."USER" AS "username", p.HOST AS "host", p.COMMAND AS "command",
       p.STATE AS "status", p.TIME AS "seconds", SUBSTR(p.INFO, 1, 200) AS "current_sql",
       'KILL ' || p.ID AS "kill_sql"
FROM V$OB_PROCESSLIST p
ORDER BY p.TIME DESC

-- [running]
-- name.zh-CN: 正在执行的语句
-- name.en: Running Statements
-- category: session
SELECT p.ID AS "sid", p."USER" AS "username", p.HOST AS "host", p.TIME AS "seconds",
       p.TRANS_STATE AS "trans_state", SUBSTR(p.INFO, 1, 500) AS "current_sql",
       'KILL ' || p.ID AS "kill_sql"
FROM V$OB_PROCESSLIST p
WHERE p.COMMAND <> 'Sleep' AND p.ID <> SYS_CONTEXT('USERENV', 'SID')
ORDER BY p.TIME DESC

-- [locks]
-- name.zh-CN: 锁等待
-- name.en: Lock Waits
-- category: lock
SELECT w.SESSION_ID AS "waiting_sid", SUBSTR(wp.INFO, 1, 200) AS "waiting_sql",
       h.SESSION_ID AS "blocking_sid", SUBSTR(hp.INFO, 1, 200) AS "blocking_sql",
       w.TYPE AS "lock_type", w.CTIME AS "wait_seconds",
       'KILL ' || h.SESSION_ID AS "kill_sql"
FROM V$OB_LOCKS w
LEFT JOIN V$OB_LOCKS h ON h.TRANS_ID = w.ID1 AND h.BLOCK = 0
LEFT JOIN V$OB_PROCESSLIST wp ON wp.ID = w.SESSION_ID
LEFT JOIN V$OB_PROCESSLIST hp ON hp.ID = h.SESSION_ID
WHERE w.BLOCK = 1
ORDER BY w.CTIME DESC

-- [storage]
-- name.zh-CN: 租户存储
-- name.en: Tenant Storage
-- category: storage
SELECT u.SVR_IP AS "server",
       ROUND(u.DATA_DISK_IN_USE / 1048576, 2) AS "data_used_mb",
       ROUND(u.LOG_DISK_IN_USE / 1048576, 2) AS "log_used_mb",
       ROUND(u.LOG_DISK_SIZE / 1048576, 2) AS "log_total_mb",
       ROUND(u.MEMORY_SIZE / 1048576, 2) AS "memory_mb"
FROM V$OB_UNITS u
ORDER BY 1

-- [slow_sql]
-- name.zh-CN: 慢 SQL
-- name.en: Slow SQL
-- category: sql
SELECT * FROM (
  SELECT SUBSTR(p.QUERY_SQL, 1, 300) AS "sql_text", p.EXECUTIONS AS "executions",
         ROUND(p.AVG_EXE_USEC / 1000, 2) AS "avg_ms",
         ROUND(p.SLOWEST_EXE_USEC / 1000, 2) AS "max_ms",
         ROUND(p.ELAPSED_TIME / 1000, 2) AS "total_ms",
         p.ROWS_PROCESSED AS "rows_processed", p.LAST_ACTIVE_TIME AS "last_seen"
  FROM GV$OB_PLAN_CACHE_PLAN_STAT p
  ORDER BY p.AVG_EXE_USEC DESC
) WHERE ROWNUM <= 20
