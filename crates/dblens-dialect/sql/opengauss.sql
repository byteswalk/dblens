-- openGauss overrides on top of postgres.sql (only keys that differ). Based on PG 9.2; tested on 7.0.0-RC1
-- with databases in A (Oracle-compatible) and PG compatibility.

-- [variant_probe]
-- Compatibility of the current database (A / B / C / PG) and the PostgreSQL version the build is based on.
SELECT datcompatibility AS mode,
       current_setting('server_version_num') AS version_num
FROM pg_catalog.pg_database
WHERE datname = current_database()
