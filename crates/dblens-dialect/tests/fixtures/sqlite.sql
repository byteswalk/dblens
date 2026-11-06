-- SQLite 元数据测试对象（main 库），外加 ATTACH 的 aux 库。

-- @@ dept
CREATE TABLE dept (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  name VARCHAR(50) NOT NULL CONSTRAINT uq_dept_name UNIQUE,
  budget DECIMAL(12,2) DEFAULT 0 CHECK (budget >= 0)
);

-- @@ emp
CREATE TABLE "emp" (
  id INTEGER NOT NULL,
  seq INT NOT NULL,
  dept_id INTEGER CONSTRAINT fk_emp_dept REFERENCES dept ON DELETE CASCADE,
  name TEXT NOT NULL DEFAULT '无名',
  salary REAL,
  active BOOLEAN DEFAULT 1,
  hired DATE DEFAULT (date('now')),
  updated DATETIME,
  doc JSON,
  photo BLOB,
  untyped,
  name_upper TEXT GENERATED ALWAYS AS (upper(name)) VIRTUAL,
  double_salary REAL AS (salary * 2) STORED,
  CONSTRAINT pk_emp PRIMARY KEY (id, seq),
  UNIQUE (dept_id, name),
  CONSTRAINT ck_salary CHECK (salary IS NULL OR salary > 0)
);

-- @@ idx
CREATE INDEX idx_emp_name ON emp (lower(name) DESC, hired);

-- @@ kv
CREATE TABLE kv (k TEXT PRIMARY KEY, v BLOB) WITHOUT ROWID;

-- @@ view
CREATE VIEW v_emp AS SELECT e.id, e.name, d.name AS dept FROM emp e JOIN dept d ON d.id = e.dept_id;

-- @@ trigger
CREATE TRIGGER trg_emp_au AFTER UPDATE OF salary ON emp
BEGIN
  UPDATE emp SET active = CASE WHEN NEW.salary > 0 THEN 1 ELSE 0 END WHERE id = NEW.id;
END;

-- @@ attach
ATTACH DATABASE ':memory:' AS aux;

-- @@ aux table
CREATE TABLE aux.other (id INTEGER PRIMARY KEY, note TEXT);
