-- MariaDB overrides of the MySQL ops queries (M1c3). MariaDB has no sys schema; lock waits come from
-- the InnoDB tables in information_schema.

-- [locks]
-- name.zh-CN: 锁等待
-- name.en: Lock Waits
-- category: lock
SELECT r.trx_mysql_thread_id AS waiting_pid, LEFT(r.trx_query, 200) AS waiting_sql,
       b.trx_mysql_thread_id AS blocking_pid, LEFT(b.trx_query, 200) AS blocking_sql,
       l.lock_table AS locked_table,
       TIMESTAMPDIFF(SECOND, r.trx_wait_started, NOW()) AS wait_seconds,
       CONCAT('KILL ', b.trx_mysql_thread_id) AS kill_sql
FROM information_schema.INNODB_LOCK_WAITS w
JOIN information_schema.INNODB_TRX r ON r.trx_id = w.requesting_trx_id
JOIN information_schema.INNODB_TRX b ON b.trx_id = w.blocking_trx_id
LEFT JOIN information_schema.INNODB_LOCKS l ON l.lock_id = w.requested_lock_id
ORDER BY wait_seconds DESC
