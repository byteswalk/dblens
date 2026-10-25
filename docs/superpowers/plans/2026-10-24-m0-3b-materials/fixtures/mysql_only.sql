-- 只在 MySQL 上执行（MariaDB 没有函数索引）
-- @@ idx_func
CREATE INDEX idx_emp_upper_name ON dblens_meta.emp ((UPPER(name)))
-- @@ uq_func
CREATE TABLE dblens_meta.uq_func (id INT NOT NULL, a VARCHAR(20), b VARCHAR(20), PRIMARY KEY (id), UNIQUE KEY uq_func_expr ((LOWER(a))), UNIQUE KEY uq_func_mixed (b, (LOWER(a))))
