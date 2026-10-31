-- @@ drop_synonym
DROP SYNONYM IF EXISTS dm_syn_emp
-- @@ drop_view
DROP VIEW IF EXISTS dm_v_emp
-- @@ drop_proc
DROP PROCEDURE IF EXISTS dm_p_meta
-- @@ drop_func
DROP FUNCTION IF EXISTS dm_f_meta
-- @@ drop_emp
DROP TABLE IF EXISTS dm_emp
-- @@ drop_dept
DROP TABLE IF EXISTS dm_dept
-- @@ drop_seq
DROP SEQUENCE IF EXISTS dm_seq
-- @@ dept
CREATE TABLE dm_dept (id SERIAL PRIMARY KEY, name VARCHAR(50) NOT NULL UNIQUE, note LVARCHAR(2000))
-- @@ dept_c
COMMENT ON TABLE dm_dept IS '部门表'
-- @@ seq
CREATE SEQUENCE dm_seq START WITH 100 INCREMENT BY 5 MAXVALUE 99999 CYCLE CACHE 10
-- @@ emp
CREATE TABLE dm_emp (
  id BIGSERIAL PRIMARY KEY,
  dept_id INT NOT NULL REFERENCES dm_dept (id) ON DELETE CASCADE,
  manager_id BIGINT,
  name VARCHAR(100) DEFAULT '无名' NOT NULL,
  salary DECIMAL(10,2) CHECK (salary >= 0 AND salary < 99999999),
  active BOOLEAN,
  created DATETIME YEAR TO FRACTION(3),
  clock DATETIME HOUR TO SECOND,
  bio TEXT,
  photo BYTE,
  UNIQUE (dept_id, name)
)
-- @@ emp_c
COMMENT ON TABLE dm_emp IS '员工表'
-- @@ name_c
COMMENT ON COLUMN dm_emp.name IS '员工姓名'
-- @@ emp_fk
ALTER TABLE dm_emp ADD CONSTRAINT FOREIGN KEY (manager_id) REFERENCES dm_emp (id)
-- @@ idx
CREATE INDEX idx_dm_emp ON dm_emp (created DESC, name)
-- @@ view
CREATE VIEW dm_v_emp (id, name, dept) AS SELECT e.id, e.name, d.name FROM dm_emp e JOIN dm_dept d ON d.id = e.dept_id
-- @@ proc
CREATE PROCEDURE dm_p_meta(n INT)
  DEFINE i INT;
  LET i = n + 1;
END PROCEDURE
-- @@ func
CREATE FUNCTION dm_f_meta(n INT) RETURNING INT;
  RETURN n * 2;
END FUNCTION
-- @@ trigger
CREATE TRIGGER dm_trg UPDATE OF salary ON dm_emp FOR EACH ROW (UPDATE dm_dept SET note = 'x' WHERE id = 0)
-- @@ synonym
CREATE SYNONYM dm_syn_emp FOR dm_emp
