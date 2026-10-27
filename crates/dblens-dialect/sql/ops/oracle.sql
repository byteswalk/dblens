-- Oracle ops queries (M1c3). They read V$ and DBA_ views: users without SELECT_CATALOG_ROLE (or
-- equivalent grants) get the database's own error. Aliases are quoted to keep lower-case column names.

-- [sessions]
-- name.zh-CN: 会话列表
-- name.en: Sessions
-- category: session
SELECT s.SID AS "sid", s.SERIAL# AS "serial", s.USERNAME AS "username", s.MACHINE AS "host",
       s.PROGRAM AS "program", s.STATUS AS "status", s.LAST_CALL_ET AS "seconds",
       SUBSTR(q.SQL_TEXT, 1, 200) AS "current_sql",
       'ALTER SYSTEM KILL SESSION ''' || s.SID || ',' || s.SERIAL# || ''' IMMEDIATE' AS "kill_sql"
FROM V$SESSION s
LEFT JOIN V$SQL q ON q.SQL_ID = s.SQL_ID AND q.CHILD_NUMBER = s.SQL_CHILD_NUMBER
WHERE s.TYPE = 'USER'
ORDER BY s.LAST_CALL_ET DESC

-- [running]
-- name.zh-CN: 正在执行的语句
-- name.en: Running Statements
-- category: session
SELECT s.SID AS "sid", s.SERIAL# AS "serial", s.USERNAME AS "username", s.MACHINE AS "host",
       s.LAST_CALL_ET AS "seconds", s.EVENT AS "wait_event", SUBSTR(q.SQL_TEXT, 1, 500) AS "current_sql",
       'ALTER SYSTEM KILL SESSION ''' || s.SID || ',' || s.SERIAL# || ''' IMMEDIATE' AS "kill_sql"
FROM V$SESSION s
LEFT JOIN V$SQL q ON q.SQL_ID = s.SQL_ID AND q.CHILD_NUMBER = s.SQL_CHILD_NUMBER
WHERE s.TYPE = 'USER' AND s.STATUS = 'ACTIVE' AND s.SID <> SYS_CONTEXT('USERENV', 'SID')
ORDER BY s.LAST_CALL_ET DESC

-- [locks]
-- name.zh-CN: 锁等待
-- name.en: Lock Waits
-- category: lock
SELECT w.SID AS "waiting_sid", SUBSTR(wq.SQL_TEXT, 1, 200) AS "waiting_sql",
       b.SID AS "blocking_sid", b.SERIAL# AS "blocking_serial", SUBSTR(bq.SQL_TEXT, 1, 200) AS "blocking_sql",
       o.OWNER || '.' || o.OBJECT_NAME AS "locked_object", w.SECONDS_IN_WAIT AS "wait_seconds",
       'ALTER SYSTEM KILL SESSION ''' || b.SID || ',' || b.SERIAL# || ''' IMMEDIATE' AS "kill_sql"
FROM V$SESSION w
JOIN V$SESSION b ON b.SID = w.BLOCKING_SESSION
LEFT JOIN V$SQL wq ON wq.SQL_ID = w.SQL_ID AND wq.CHILD_NUMBER = w.SQL_CHILD_NUMBER
LEFT JOIN V$SQL bq ON bq.SQL_ID = b.PREV_SQL_ID AND bq.CHILD_NUMBER = b.PREV_CHILD_NUMBER
LEFT JOIN ALL_OBJECTS o ON o.OBJECT_ID = w.ROW_WAIT_OBJ#
WHERE w.BLOCKING_SESSION IS NOT NULL
ORDER BY w.SECONDS_IN_WAIT DESC

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
SELECT * FROM (
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
) WHERE ROWNUM <= 20

-- [slow_sql]
-- name.zh-CN: 慢 SQL
-- name.en: Slow SQL
-- category: sql
SELECT * FROM (
  SELECT q.SQL_ID AS "sql_id", q.PARSING_SCHEMA_NAME AS "schema", SUBSTR(q.SQL_TEXT, 1, 300) AS "sql_text",
         q.EXECUTIONS AS "executions", ROUND(q.ELAPSED_TIME / 1000 / q.EXECUTIONS, 2) AS "avg_ms",
         ROUND(q.ELAPSED_TIME / 1000, 2) AS "total_ms", q.BUFFER_GETS AS "buffer_gets",
         q.LAST_ACTIVE_TIME AS "last_seen"
  FROM V$SQL q
  WHERE q.EXECUTIONS > 0
  ORDER BY q.ELAPSED_TIME / q.EXECUTIONS DESC
) WHERE ROWNUM <= 20
