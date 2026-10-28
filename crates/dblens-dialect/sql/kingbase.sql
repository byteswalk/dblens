-- KingbaseES overrides on top of postgres.sql (only keys that differ). Tested on V9R2C16 in Oracle and PG mode.

-- [variant_probe]
-- Compatibility mode (oracle / pg / mysql / sqlserver) and the PostgreSQL version the build is based on.
SELECT current_setting('database_mode') AS mode,
       current_setting('server_version_num') AS version_num
