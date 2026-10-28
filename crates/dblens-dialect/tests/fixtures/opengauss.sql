-- openGauss（PG 与 A 兼容模式通用）：postgres.sql 的改写版。没有 IDENTITY 列，改用 serial；存储过程用 openGauss 的写法；
-- 分区在建表时一并定义，LIST 分区键不能是 text；没有 EXCLUDE 约束；numeric 的小数位不能为负。
-- @@ drop_schema
DROP SCHEMA IF EXISTS dblens_meta CASCADE
-- @@ create_schema
CREATE SCHEMA dblens_meta
-- @@ dept
CREATE TABLE dblens_meta.dept (id serial PRIMARY KEY, name varchar(50) NOT NULL UNIQUE, note text)
-- @@ dept_c1
COMMENT ON TABLE dblens_meta.dept IS '部门表'
-- @@ dept_c2
COMMENT ON COLUMN dblens_meta.dept.id IS '部门编号'
-- @@ dept_c3
COMMENT ON COLUMN dblens_meta.dept.name IS '部门名称'
-- @@ seq_meta
CREATE SEQUENCE dblens_meta.seq_meta START 100 INCREMENT 5 MAXVALUE 99999 CYCLE CACHE 10
-- @@ emp
CREATE TABLE dblens_meta.emp (
  id bigint PRIMARY KEY,
  dept_id integer,
  manager_id bigint,
  name varchar(100) NOT NULL DEFAULT '无名',
  salary numeric(10,2) CONSTRAINT ck_emp_salary CHECK (salary >= 0),
  created timestamp(6) DEFAULT CURRENT_TIMESTAMP,
  bio text,
  photo bytea,
  tags text[],
  serial_no serial,
  name_upper text GENERATED ALWAYS AS (upper(name)) STORED,
  CONSTRAINT fk_emp_dept FOREIGN KEY (dept_id) REFERENCES dblens_meta.dept(id) ON DELETE CASCADE,
  CONSTRAINT fk_emp_mgr FOREIGN KEY (manager_id) REFERENCES dblens_meta.emp(id) ON DELETE SET NULL ON UPDATE NO ACTION,
  CONSTRAINT uq_emp_dept_name UNIQUE (dept_id, name))
-- @@ emp_c1
COMMENT ON TABLE dblens_meta.emp IS '员工表 employees'
-- @@ emp_c2
COMMENT ON COLUMN dblens_meta.emp.name IS '员工姓名'
-- @@ idx_created
CREATE INDEX idx_emp_created ON dblens_meta.emp (created DESC)
-- @@ idx_upper
CREATE INDEX idx_emp_upper ON dblens_meta.emp (lower(name), dept_id)
-- @@ idx_gin
CREATE INDEX idx_emp_tags ON dblens_meta.emp USING gin (tags)
-- @@ idx_partial
CREATE INDEX idx_emp_partial ON dblens_meta.emp (salary) WHERE salary > 1000
-- @@ trg_fn
CREATE FUNCTION dblens_meta.trg_emp() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN RETURN NEW; END $$
-- @@ trg
CREATE TRIGGER trg_emp_biu BEFORE INSERT OR UPDATE ON dblens_meta.emp FOR EACH ROW EXECUTE FUNCTION dblens_meta.trg_emp()
-- @@ trg2
CREATE TRIGGER trg_emp_trunc AFTER TRUNCATE ON dblens_meta.emp FOR EACH STATEMENT EXECUTE FUNCTION dblens_meta.trg_emp()
-- @@ v_emp
CREATE VIEW dblens_meta.v_emp AS SELECT e.id, e.name, d.name AS dept_name FROM dblens_meta.emp e LEFT JOIN dblens_meta.dept d ON d.id = e.dept_id
-- @@ v_emp_c
COMMENT ON VIEW dblens_meta.v_emp IS '员工视图'
-- @@ mv_dept
CREATE MATERIALIZED VIEW dblens_meta.mv_dept AS SELECT d.id, d.name, count(e.id) AS cnt FROM dblens_meta.dept d LEFT JOIN dblens_meta.emp e ON e.dept_id = d.id GROUP BY d.id, d.name
-- @@ mv_dept_c
COMMENT ON MATERIALIZED VIEW dblens_meta.mv_dept IS '部门统计物化视图'
-- @@ mv_dept_cc
COMMENT ON COLUMN dblens_meta.mv_dept.cnt IS '员工数'
-- @@ p_meta
CREATE PROCEDURE dblens_meta.p_meta(a integer) AS BEGIN NULL; END;
-- @@ f_meta
CREATE FUNCTION dblens_meta.f_meta(a integer) RETURNS integer LANGUAGE sql AS $$ SELECT a + 1 $$
-- @@ f_meta2
CREATE FUNCTION dblens_meta.f_meta(a text, b integer) RETURNS text LANGUAGE sql AS $$ SELECT a || b::text $$
-- @@ f_meta_c
COMMENT ON FUNCTION dblens_meta.f_meta(integer) IS '加一函数'
-- @@ agg_fn
CREATE FUNCTION dblens_meta.agg_sfunc(integer, integer) RETURNS integer LANGUAGE sql AS $$ SELECT $1 + $2 $$
-- @@ agg
CREATE AGGREGATE dblens_meta.sum_meta(integer) (SFUNC = dblens_meta.agg_sfunc, STYPE = integer)
-- @@ part_t
CREATE TABLE dblens_meta.part_t (id integer, created date NOT NULL, v text) PARTITION BY RANGE (created) (PARTITION part_t_2024 VALUES LESS THAN ('2025-01-01'), PARTITION part_t_2025 VALUES LESS THAN ('2026-01-01'))
-- @@ part_h
CREATE TABLE dblens_meta.part_h (id integer) PARTITION BY HASH (id) (PARTITION part_h0, PARTITION part_h1)
-- @@ part_l
CREATE TABLE dblens_meta.part_l (id integer, region varchar(10)) PARTITION BY LIST (region) (PARTITION part_l_a VALUES ('a'))
-- @@ inh_parent
CREATE TABLE dblens_meta.inh_parent (id integer)
-- @@ inh_child
CREATE TABLE dblens_meta.inh_child (extra integer) INHERITS (dblens_meta.inh_parent)
-- @@ temp_t
CREATE TEMP TABLE meta_tmp_probe (id integer)
-- @@ unlogged_t
CREATE UNLOGGED TABLE dblens_meta.unlogged_t (id integer)
-- @@ quoted
CREATE TABLE dblens_meta."Mixed Case" ("select" integer PRIMARY KEY, "数洞" text, "Id" integer)
-- @@ uidx
CREATE UNIQUE INDEX uidx_dept_note ON dblens_meta.dept (note) WHERE note IS NOT NULL
-- @@ enum_ty
CREATE TYPE dblens_meta.mood AS ENUM ('sad','ok','happy')
-- @@ dom_ty
CREATE DOMAIN dblens_meta.posint AS integer CHECK (VALUE > 0)
-- @@ ds_t
CREATE TABLE dblens_meta.ds_t (
  c_smallint smallint, c_int integer, c_bigint bigint,
  c_numeric numeric, c_numeric_p numeric(10,2),
  c_real real, c_double double precision, c_float3 float(3),
  c_money money,
  c_char char(10), c_char1 char, c_varchar varchar(255), c_varchar_n varchar, c_text text, c_name name, c_citext_like "char",
  c_bytea bytea,
  c_bool boolean,
  c_date date, c_time time, c_time3 time(3), c_timetz time with time zone, c_ts timestamp, c_ts6 timestamp(6), c_ts3 timestamp(3), c_tstz timestamptz, c_tstz3 timestamp(3) with time zone,
  c_interval interval, c_interval_ds interval day to second(3), c_interval_ym interval year to month, c_interval_f interval(2),
  c_uuid uuid, c_json json, c_jsonb jsonb, c_xml xml,
  c_inet inet, c_cidr cidr, c_macaddr macaddr,
  c_bit bit(1), c_bit8 bit(8), c_varbit bit varying(16),
  c_tsvector tsvector, c_tsquery tsquery,
  c_point point, c_box box, c_int4range int4range, c_tstzrange tstzrange,
  c_int_arr integer[], c_text_arr text[], c_int_arr2 integer[][], c_varchar_arr varchar(20)[], c_ts_arr timestamp(3)[], c_numeric_arr numeric(8,3)[],
  c_enum dblens_meta.mood, c_domain dblens_meta.posint, c_enum_arr dblens_meta.mood[],
  c_oid oid, c_regclass regclass, c_serial serial, c_bigserial bigserial, c_smallserial smallserial)
