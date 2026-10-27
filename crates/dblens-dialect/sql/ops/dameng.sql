-- DM (Dameng) ops queries (M1c3). They read V$ dynamic views and DBA_ views: users without the DBA role
-- (or equivalent grants) get the database's own error. Aliases are quoted to keep lower-case column names.

-- [sessions]
-- name.zh-CN: 会话列表
-- name.en: Sessions
-- category: session
SELECT s.SESS_ID AS "sess_id", s.USER_NAME AS "username", s.CLNT_IP AS "host", s.APPNAME AS "program",
       s.STATE AS "state", DATEDIFF(SS, s.LAST_RECV_TIME, SYSDATE) AS "seconds",
       SUBSTR(s.SQL_TEXT, 1, 200) AS "current_sql",
       'SP_CLOSE_SESSION(' || s.SESS_ID || ')' AS "kill_sql"
FROM V$SESSIONS s
ORDER BY 6 DESC

-- [running]
-- name.zh-CN: 正在执行的语句
-- name.en: Running Statements
-- category: session
SELECT s.SESS_ID AS "sess_id", s.USER_NAME AS "username", s.CLNT_IP AS "host",
       DATEDIFF(SS, s.LAST_RECV_TIME, SYSDATE) AS "seconds", SUBSTR(s.SQL_TEXT, 1, 500) AS "current_sql",
       'SP_CLOSE_SESSION(' || s.SESS_ID || ')' AS "kill_sql"
FROM V$SESSIONS s
WHERE s.STATE = 'ACTIVE' AND s.SESS_ID <> SESSID()
ORDER BY 4 DESC

-- [locks]
-- name.zh-CN: 锁等待
-- name.en: Lock Waits
-- category: lock
SELECT ws.SESS_ID AS "waiting_sess", SUBSTR(ws.SQL_TEXT, 1, 200) AS "waiting_sql",
       bs.SESS_ID AS "blocking_sess", SUBSTR(bs.SQL_TEXT, 1, 200) AS "blocking_sql",
       w.WAIT_TIME AS "wait_ms",
       'SP_CLOSE_SESSION(' || bs.SESS_ID || ')' AS "kill_sql"
FROM V$TRXWAIT w
JOIN V$SESSIONS ws ON ws.TRX_ID = w.ID
JOIN V$SESSIONS bs ON bs.TRX_ID = w.WAIT_FOR_ID
ORDER BY w.WAIT_TIME DESC

-- [storage]
-- name.zh-CN: 表空间使用
-- name.en: Tablespace Usage
-- category: storage
SELECT d.TABLESPACE_NAME AS "tablespace",
       ROUND(SUM(d.BYTES) / 1048576, 2) AS "total_mb",
       ROUND((SUM(d.BYTES) - NVL(MAX(f.FREE_BYTES), 0)) / 1048576, 2) AS "used_mb",
       ROUND((SUM(d.BYTES) - NVL(MAX(f.FREE_BYTES), 0)) * 100 / SUM(d.BYTES), 2) AS "used_pct",
       ROUND(SUM(CASE WHEN d.AUTOEXTENSIBLE = 'YES' THEN d.MAXBYTES ELSE d.BYTES END) / 1048576, 2) AS "max_mb"
FROM DBA_DATA_FILES d
LEFT JOIN (SELECT TABLESPACE_NAME, SUM(BYTES) AS FREE_BYTES FROM DBA_FREE_SPACE GROUP BY TABLESPACE_NAME) f
       ON f.TABLESPACE_NAME = d.TABLESPACE_NAME
GROUP BY d.TABLESPACE_NAME
ORDER BY 4 DESC

-- [largest_tables]
-- name.zh-CN: 占用最大的表
-- name.en: Largest Tables
-- category: storage
SELECT x.OWNER AS "owner", x.TABLE_NAME AS "table_name",
       ROUND(SUM(CASE WHEN x.KIND = 'T' THEN x.BYTES ELSE 0 END) / 1048576, 2) AS "data_mb",
       ROUND(SUM(CASE WHEN x.KIND = 'I' THEN x.BYTES ELSE 0 END) / 1048576, 2) AS "index_mb",
       ROUND(SUM(x.BYTES) / 1048576, 2) AS "total_mb"
FROM (
  SELECT s.OWNER, s.SEGMENT_NAME AS TABLE_NAME, 'T' AS KIND, s.BYTES
  FROM DBA_SEGMENTS s WHERE s.SEGMENT_TYPE LIKE 'TABLE%'
  UNION ALL
  SELECT i.TABLE_OWNER, i.TABLE_NAME, 'I', s.BYTES
  FROM DBA_SEGMENTS s JOIN DBA_INDEXES i ON i.OWNER = s.OWNER AND i.INDEX_NAME = s.SEGMENT_NAME
  WHERE s.SEGMENT_TYPE LIKE 'INDEX%'
) x
GROUP BY x.OWNER, x.TABLE_NAME
ORDER BY SUM(x.BYTES) DESC
LIMIT 20

-- [slow_sql]
-- name.zh-CN: 慢 SQL
-- name.en: Slow SQL
-- category: sql
SELECT l.SESS_ID AS "sess_id", SUBSTR(l.SQL_TEXT, 1, 300) AS "sql_text", l.EXEC_TIME AS "elapsed_ms",
       l.FINISH_TIME AS "finished_at"
FROM V$LONG_EXEC_SQLS l
ORDER BY l.EXEC_TIME DESC
LIMIT 20
