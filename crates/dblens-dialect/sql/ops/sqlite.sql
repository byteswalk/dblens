-- SQLite ops queries. There are no sessions, locks or statement statistics: only storage of the main database file.
-- dbstat is compiled into the bundled SQLite (SQLITE_ENABLE_DBSTAT_VTAB).

-- [storage]
-- name.zh-CN: 存储概况
-- name.en: Storage Overview
-- category: storage
SELECT d.name AS db, d.file,
       pc.page_count * ps.page_size AS total_bytes,
       fl.freelist_count * ps.page_size AS free_bytes,
       ps.page_size, pc.page_count, fl.freelist_count,
       jm.journal_mode, enc.encoding
FROM pragma_database_list d
CROSS JOIN pragma_page_size ps
CROSS JOIN pragma_page_count pc
CROSS JOIN pragma_freelist_count fl
CROSS JOIN pragma_journal_mode jm
CROSS JOIN pragma_encoding enc
WHERE d.name = 'main'

-- [largest_tables]
-- name.zh-CN: 最大的表
-- name.en: Largest Tables
-- category: storage
-- bytes = the table plus its indexes.
SELECT s.tbl_name AS table_name,
       SUM(CASE WHEN s.type = 'table' THEN st.bytes ELSE 0 END) AS table_bytes,
       SUM(CASE WHEN s.type = 'index' THEN st.bytes ELSE 0 END) AS index_bytes,
       SUM(st.bytes) AS bytes
FROM (SELECT name, SUM(pgsize) AS bytes FROM dbstat('main') GROUP BY name) st
JOIN main.sqlite_schema s ON s.name = st.name
WHERE s.tbl_name NOT LIKE 'sqlite!_%' ESCAPE '!'
GROUP BY s.tbl_name
ORDER BY bytes DESC
LIMIT 50
