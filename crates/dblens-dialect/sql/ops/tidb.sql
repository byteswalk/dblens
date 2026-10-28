-- TiDB overrides of the MySQL ops queries. There is no sys schema; lock waits come from DATA_LOCK_WAITS
-- (pessimistic transactions) with the transactions of both sides from CLUSTER_TIDB_TRX.

-- [locks]
-- name.zh-CN: 锁等待
-- name.en: Lock Waits
-- category: lock
SELECT wt.SESSION_ID AS waiting_pid, LEFT(wt.CURRENT_SQL_DIGEST_TEXT, 200) AS waiting_sql,
       bt.SESSION_ID AS blocking_pid, LEFT(bt.CURRENT_SQL_DIGEST_TEXT, 200) AS blocking_sql,
       w.KEY AS locked_key,
       TIMESTAMPDIFF(SECOND, wt.WAITING_START_TIME, NOW()) AS wait_seconds,
       CONCAT('KILL ', bt.SESSION_ID) AS kill_sql
FROM information_schema.DATA_LOCK_WAITS w
LEFT JOIN information_schema.CLUSTER_TIDB_TRX wt ON wt.ID = w.TRX_ID
LEFT JOIN information_schema.CLUSTER_TIDB_TRX bt ON bt.ID = w.CURRENT_HOLDING_TRX_ID
ORDER BY wait_seconds DESC
