-- GoldenDB overrides. The tenant account has no rights on the sys views, so lock waits come from the base tables.

-- [locks]
-- name.zh-CN: 锁等待
-- name.en: Lock Waits
-- category: lock
SELECT r.trx_mysql_thread_id AS waiting_pid, LEFT(r.trx_query, 200) AS waiting_sql,
       b.trx_mysql_thread_id AS blocking_pid, LEFT(b.trx_query, 200) AS blocking_sql,
       l.OBJECT_NAME AS locked_table, TIMESTAMPDIFF(SECOND, r.trx_wait_started, NOW()) AS wait_seconds,
       CONCAT('KILL ', b.trx_mysql_thread_id) AS kill_sql
FROM performance_schema.data_lock_waits w
JOIN information_schema.INNODB_TRX r ON r.trx_id = w.REQUESTING_ENGINE_TRANSACTION_ID
JOIN information_schema.INNODB_TRX b ON b.trx_id = w.BLOCKING_ENGINE_TRANSACTION_ID
JOIN performance_schema.data_locks l ON l.ENGINE_LOCK_ID = w.REQUESTING_ENGINE_LOCK_ID
ORDER BY wait_seconds DESC
