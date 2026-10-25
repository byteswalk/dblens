-- @@ drop_db
DROP DATABASE IF EXISTS dblens_meta
-- @@ create_db
CREATE DATABASE dblens_meta DEFAULT CHARACTER SET utf8mb4
-- @@ dept
CREATE TABLE dblens_meta.dept (id INT NOT NULL AUTO_INCREMENT COMMENT '部门编号', name VARCHAR(50) NOT NULL COMMENT '部门名称', PRIMARY KEY (id), UNIQUE KEY uk_dept_name (name)) COMMENT='部门表'
-- @@ emp
CREATE TABLE dblens_meta.emp (id BIGINT NOT NULL, dept_id INT NULL, manager_id BIGINT NULL, name VARCHAR(100) NOT NULL DEFAULT '无名' COMMENT '员工姓名', salary DECIMAL(10,2) NULL, created TIMESTAMP(6) NOT NULL DEFAULT CURRENT_TIMESTAMP(6), bio TEXT NULL, photo BLOB NULL, name_upper VARCHAR(100) GENERATED ALWAYS AS (UPPER(name)) VIRTUAL, PRIMARY KEY (id), UNIQUE KEY uk_emp_dept_name (dept_id, name), CONSTRAINT fk_emp_dept FOREIGN KEY (dept_id) REFERENCES dblens_meta.dept (id) ON DELETE CASCADE, CONSTRAINT fk_emp_mgr FOREIGN KEY (manager_id) REFERENCES dblens_meta.emp (id) ON DELETE SET NULL, CONSTRAINT ck_emp_salary CHECK (salary >= 0)) COMMENT='员工表'
-- @@ idx_created
CREATE INDEX idx_emp_created ON dblens_meta.emp (created DESC)
-- @@ idx_fulltext
CREATE FULLTEXT INDEX ft_emp_bio ON dblens_meta.emp (bio)
-- @@ trg
CREATE TRIGGER dblens_meta.trg_emp_bi BEFORE INSERT ON dblens_meta.emp FOR EACH ROW SET NEW.name = TRIM(NEW.name)
-- @@ view
CREATE VIEW dblens_meta.v_emp AS SELECT e.id, e.name, d.name AS dept_name FROM dblens_meta.emp e LEFT JOIN dblens_meta.dept d ON d.id = e.dept_id
-- @@ proc
CREATE PROCEDURE dblens_meta.p_meta(IN p_id INT) COMMENT '测试过程' SELECT p_id
-- @@ func
CREATE FUNCTION dblens_meta.f_meta(p_a INT) RETURNS INT DETERMINISTIC COMMENT '测试函数' RETURN p_a + 1
-- @@ part_t
CREATE TABLE dblens_meta.part_t (id INT NOT NULL, created DATE NOT NULL, PRIMARY KEY (id, created)) PARTITION BY RANGE (YEAR(created)) (PARTITION p2023 VALUES LESS THAN (2024), PARTITION p2024 VALUES LESS THAN (2025), PARTITION pmax VALUES LESS THAN MAXVALUE)
-- @@ mixed
CREATE TABLE dblens_meta.`Mixed Case` (`select` INT NOT NULL, `数洞` VARCHAR(20) NULL, `a b` INT NULL, PRIMARY KEY (`select`))
-- @@ ds_t
CREATE TABLE dblens_meta.ds_t (c_tinyint1 TINYINT(1), c_tinyint TINYINT, c_bit1 BIT(1), c_bit8 BIT(8), c_year YEAR, c_smallint SMALLINT, c_mediumint_u MEDIUMINT UNSIGNED, c_int INT, c_int_u INT UNSIGNED, c_bigint_u BIGINT UNSIGNED, c_float FLOAT, c_double DOUBLE, c_decimal DECIMAL(65,30), c_decimal_d DECIMAL(10,2), c_char CHAR(10), c_varchar VARCHAR(255) CHARACTER SET utf8mb4 COLLATE utf8mb4_bin, c_tinytext TINYTEXT, c_text TEXT, c_mediumtext MEDIUMTEXT, c_longtext LONGTEXT, c_binary BINARY(16), c_varbinary VARBINARY(100), c_tinyblob TINYBLOB, c_blob BLOB, c_mediumblob MEDIUMBLOB, c_longblob LONGBLOB, c_date DATE, c_time3 TIME(3), c_datetime6 DATETIME(6), c_datetime DATETIME, c_timestamp TIMESTAMP NULL, c_json JSON, c_enum ENUM('a','b','it''s'), c_set SET('x','y','z'), c_geometry GEOMETRY)
-- @@ def_t
CREATE TABLE dblens_meta.def_t (a INT DEFAULT 0, b VARCHAR(20) DEFAULT 'it''s \\ x', c DATETIME DEFAULT CURRENT_TIMESTAMP, d INT DEFAULT NULL, e VARCHAR(40) DEFAULT (UUID()), f BIT(1) DEFAULT b'1', g DATE DEFAULT '2020-01-01', h DECIMAL(5,2) DEFAULT 1.50, i TIMESTAMP NULL DEFAULT NULL ON UPDATE CURRENT_TIMESTAMP, j VARCHAR(5) DEFAULT 'NULL', k INT NOT NULL)
-- @@ ins
INSERT INTO dblens_meta.dept (name) VALUES ('研发')
