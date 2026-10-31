-- GBase 8s ops queries, from sysmaster. Killing a session goes through the sysadmin task() function.

-- [sessions]
-- name.zh-CN: 会话列表
-- name.en: Sessions
-- category: session
SELECT s.sid AS session_id, TRIM(s.username) AS "user", TRIM(s.hostname) AS host, TRIM(q.sqs_dbname) AS db,
       TRIM(s.feprogram) AS application, DBINFO('utc_current') - s.connected AS seconds,
       q.sqs_statement AS current_sql,
       'EXECUTE FUNCTION sysadmin:task(''onmode'', ''z'', ''' || s.sid || ''')' AS kill_sql
FROM sysmaster:syssessions s
LEFT JOIN sysmaster:syssqlstat q ON q.sqs_sessionid = s.sid
ORDER BY s.connected DESC

-- [running]
-- name.zh-CN: 正在执行的语句
-- name.en: Running Statements
-- category: session
SELECT q.sqs_sessionid AS session_id, TRIM(s.username) AS "user", TRIM(s.hostname) AS host,
       TRIM(q.sqs_dbname) AS db, q.sqs_statement AS current_sql,
       'EXECUTE FUNCTION sysadmin:task(''onmode'', ''z'', ''' || q.sqs_sessionid || ''')' AS kill_sql
FROM sysmaster:syssqlstat q
JOIN sysmaster:syssessions s ON s.sid = q.sqs_sessionid
WHERE q.sqs_statement IS NOT NULL AND q.sqs_sessionid <> DBINFO('sessionid')
ORDER BY q.sqs_sessionid

-- [locks]
-- name.zh-CN: 锁等待
-- name.en: Lock Waits
-- category: lock
SELECT l.waiter AS waiting_session, l.owner AS blocking_session, TRIM(l.dbsname) AS db,
       TRIM(l.tabname) AS table_name, l.type AS lock_type,
       'EXECUTE FUNCTION sysadmin:task(''onmode'', ''z'', ''' || l.owner || ''')' AS kill_sql
FROM sysmaster:syslocks l
WHERE l.waiter IS NOT NULL
ORDER BY l.owner

-- [storage]
-- name.zh-CN: 各空间大小
-- name.en: Dbspace Sizes
-- category: storage
SELECT TRIM(d.name) AS name, SUM(c.chksize * c.pagesize) AS total_bytes, SUM(c.nfree * c.pagesize) AS free_bytes
FROM sysmaster:sysdbspaces d
JOIN sysmaster:syschunks c ON c.dbsnum = d.dbsnum
GROUP BY d.name
ORDER BY 1

-- [largest_tables]
-- name.zh-CN: 最大的表
-- name.en: Largest Tables
-- category: storage
SELECT FIRST 50 TRIM(n.dbsname) AS db, TRIM(n.owner) AS schema_name, TRIM(n.tabname) AS table_name,
       p.nptotal * p.pagesize AS bytes, p.nrows AS row_count
FROM sysmaster:sysptnhdr p
JOIN sysmaster:systabnames n ON n.partnum = p.partnum
WHERE n.dbsname NOT IN ('sysmaster', 'sysadmin', 'sysuser', 'sysutils')
ORDER BY p.nptotal * p.pagesize DESC
