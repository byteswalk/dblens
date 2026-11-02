-- GoldenDB overrides. information_schema.TRIGGERS has no user triggers here, only SHOW TRIGGERS lists them.
-- SHOW takes no bound schema; Dialect::adapt_rows turns its columns into the ones `triggers` must return.

-- [triggers]
SHOW TRIGGERS FROM {{ schema_ident }}
