-- @@ drop_objects
DROP SYNONYM IF EXISTS dblens_meta.syn_emp;
DROP VIEW IF EXISTS dblens_meta.v_emp;
DROP PROCEDURE IF EXISTS dblens_meta.p_meta;
DROP FUNCTION IF EXISTS dblens_meta.f_meta;
DROP TABLE IF EXISTS dblens_meta.emp;
DROP TABLE IF EXISTS dblens_meta.dept;
DROP TABLE IF EXISTS dblens_meta.[Mixed Case];
DROP TABLE IF EXISTS dblens_meta.ds_t;
DROP SEQUENCE IF EXISTS dblens_meta.seq_meta;
DROP SCHEMA IF EXISTS dblens_meta
-- @@ create_schema
CREATE SCHEMA dblens_meta
-- @@ dept
CREATE TABLE dblens_meta.dept (id int IDENTITY(1,1) PRIMARY KEY, name nvarchar(50) NOT NULL UNIQUE, note nvarchar(max))
-- @@ dept_c
EXEC sys.sp_addextendedproperty N'MS_Description', N'部门表', N'SCHEMA', N'dblens_meta', N'TABLE', N'dept'
-- @@ seq_meta
CREATE SEQUENCE dblens_meta.seq_meta AS bigint START WITH 100 INCREMENT BY 5 MAXVALUE 99999 CYCLE CACHE 10
-- @@ emp
CREATE TABLE dblens_meta.emp (
  id bigint PRIMARY KEY,
  dept_id int,
  manager_id bigint,
  name nvarchar(100) NOT NULL CONSTRAINT df_emp_name DEFAULT N'无名',
  salary decimal(10,2) CONSTRAINT ck_emp_salary CHECK (salary >= 0),
  created datetime2(6) DEFAULT SYSDATETIME(),
  bio nvarchar(max),
  photo varbinary(max),
  name_upper AS UPPER(name),
  CONSTRAINT fk_emp_dept FOREIGN KEY (dept_id) REFERENCES dblens_meta.dept(id) ON DELETE CASCADE,
  CONSTRAINT fk_emp_mgr FOREIGN KEY (manager_id) REFERENCES dblens_meta.emp(id),
  CONSTRAINT uq_emp_dept_name UNIQUE (dept_id, name))
-- @@ emp_c1
EXEC sys.sp_addextendedproperty N'MS_Description', N'员工表 employees', N'SCHEMA', N'dblens_meta', N'TABLE', N'emp'
-- @@ emp_c2
EXEC sys.sp_addextendedproperty N'MS_Description', N'员工姓名', N'SCHEMA', N'dblens_meta', N'TABLE', N'emp', N'COLUMN', N'name'
-- @@ idx_created
CREATE INDEX idx_emp_created ON dblens_meta.emp (created DESC)
-- @@ idx_salary
CREATE INDEX idx_emp_salary ON dblens_meta.emp (salary) INCLUDE (bio) WHERE salary > 1000
-- @@ trg
CREATE TRIGGER dblens_meta.trg_emp_aiu ON dblens_meta.emp AFTER INSERT, UPDATE AS BEGIN SET NOCOUNT ON; END
-- @@ v_emp
CREATE VIEW dblens_meta.v_emp AS SELECT e.id, e.name, d.name AS dept_name FROM dblens_meta.emp e LEFT JOIN dblens_meta.dept d ON d.id = e.dept_id
-- @@ p_meta
CREATE PROCEDURE dblens_meta.p_meta @a int AS BEGIN SET NOCOUNT ON; END
-- @@ f_meta
CREATE FUNCTION dblens_meta.f_meta(@a int) RETURNS int AS BEGIN RETURN @a + 1; END
-- @@ syn_emp
CREATE SYNONYM dblens_meta.syn_emp FOR dblens_meta.emp
-- @@ mixed
CREATE TABLE dblens_meta.[Mixed Case] ([数洞] nvarchar(10), [select] int NOT NULL PRIMARY KEY)
-- @@ ds_t
CREATE TABLE dblens_meta.ds_t (
  c_bit bit,
  c_tinyint tinyint,
  c_money money,
  c_varchar_max varchar(max),
  c_nchar nchar(4),
  c_time time(3),
  c_dto datetimeoffset(7),
  c_uid uniqueidentifier,
  c_xml xml,
  c_geo geography,
  c_rv rowversion)
-- @@ rows
INSERT INTO dblens_meta.dept (name) VALUES (N'研发'), (N'市场')
