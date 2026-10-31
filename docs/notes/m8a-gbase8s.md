# M8a GBase 8s 和 JDBC 桥接

## 结构

```text
dblens（Rust）── stdin/stdout 帧 ──> dblens-bridge（Java 进程）── JDBC ──> 数据库
```

- `bridge/`：Java 进程，没有第三方依赖。全应用一个进程，第一次连接时启动，stdin 关闭时退出。
- `crates/dblens-driver-jdbc`：帧协议（`frame.rs`，Java 端是 `Frame.java`，两边要同时改）、进程管理、连接、各数据库的适配。
- 驱动 jar 用独立的类加载器，从连接选项 `jdbc_dir` 或程序自带的 `clients/jdbc/` 加载。

## 哪些数据库走桥接

| 数据库 | 方式 | 驱动 jar |
|---|---|---|
| GBase 8s | 只走桥接 | 用户放置，在设置中确认目录 |
| Oracle | 连接选项 `client=jdbc` 时走桥接 | `ojdbc11.jar` 随包发布 |
| 达梦 | 连接选项 `client=jdbc` 时走桥接 | 用户放置 `DmJdbcDriver*.jar` |

## GBase 8s 的坑

- URL 必须带 `SQLMODE=gbase`。不带时驱动按 Oracle 模式，`sysmaster:` 前缀和 `CREATE TEMP TABLE` 都报 `-201`。
- URL 必须带 `CLIENT_LOCALE`、`DB_LOCALE`。实例自带的旧 jar 不带时报内部异常。
- 必须带 `DELIMIDENT=y`，否则双引号是字符串。
- 实例里 `bin/plugins` 下的 2015 年 `ifxjdbc.jar` 不能用：`VARCHAR` 中文读成空值，读 `TEXT` 卡死。用官方的 `gbasedbtjdbc`。
- `SELECT` 必须有 `FROM`，常量查询用 `sysmaster:sysdual`。
- 约束名写在定义之后：`FOREIGN KEY (…) REFERENCES … CONSTRAINT 名字`。
- `systables` 里 `tabid >= 100` 不都是用户对象：`GL_COLLATE`、`GL_CTYPE`、`VERSION` 的 `tabid` 是 990 以上，`tabtype` 为空。
- 视图、过程、检查约束的文本按 `CHAR` 块存放，一块一行，由 `assemble.rs` 拼接。
- 约束名、索引的列在 `sysindexes.part1..16`，负数表示降序；模板用循环展开。
- 类型在 `syscolumns.coltype`、`collength` 里是编码，由 `meta/coltype.rs` 解码，高位 256 表示非空。

## 经 JDBC 时的差异

- 没有 SQL 出错位置，`Error::Sql` 的 `position` 为空。
- Oracle 收不到 `DBMS_OUTPUT`。
- 大字段按字符流读，`total_len` 是按 UTF-8 算出来的字节数。
- 没有参数的语句用 `Statement` 执行并关闭转义处理，否则驱动会改写 `{ }`。

## 实测（2026-10-30，Windows，JDK 25）

- 连接约 100 ms。
- jlink 精简 JRE 50 MB（含三种驱动所需模块），一致性测试在上面通过。
- GBase 8s：一致性 11 项、元数据、服务层 7 项、接口层通过，界面已用真实鼠标走过。
- Oracle 26ai（ojdbc11 23.26.4）：一致性 10 项、服务层 4 项通过。
- 达梦 DM8（DmJdbcDriver8）：一致性 10 项、服务层 3 项通过，用的是 VM 上 5237 端口的独立测试实例。

## 未实测

- 安装包整体安装、Linux 上的桥接、macOS。
- `DATETIME YEAR TO FRACTION(3)` 用 `CURRENT` 取回毫秒是 `.0`，未确认是服务器行为还是驱动截断。
- 打包要用 OpenJDK 构建。本机的 JDK 是 Oracle JDK，只用来量体积。
