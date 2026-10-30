# 打了补丁的第三方库

工作区 `Cargo.toml` 的 `[patch.crates-io]` 用这里的副本替换 crates.io 上的同名库。副本是 crates.io 发布包的原样内容（只删去了 `Cargo.toml` 中的 `[[test]]`、`[[bench]]`、`[[example]]`），再加上下面列出的改动。目录在 `[workspace] exclude` 中，不是工作区成员，clippy 不检查。

## openGauss 的 SHA256 认证

openGauss 用认证请求码 10（与 PostgreSQL 的 SASL 相同）发它自己的 SHA256 认证，上游的 tokio-postgres 解析失败（"error parsing response from server"）。算法与报文格式见 `postgres-protocol/src/authentication/gauss.rs` 开头的说明。

| 库 | 上游版本 | 改动 |
|---|---|---|
| `postgres-protocol` | 0.6.12 | 新增 `src/authentication/gauss.rs`（解析请求、计算证明，含单元测试）；`src/authentication/mod.rs` 加 `pub mod gauss;`；`src/message/backend.rs` 中 `AuthenticationSaslBody` 加 `data()`；`Cargo.toml` 加依赖 `sha1 = "0.11"` |
| `tokio-postgres` | 0.7.18 | `src/connect_raw.rs` 的 `authenticate`：请求码 10 的报文是 openGauss 格式时走 SHA256（或 MD5 存储时走 MD5）；请求码 11 开头的认证（openGauss 的 MD5_SHA256）直接报错说明不支持 |

不支持的情况：pg_hba 配成 md5、而密码只存了 SHA256 时，openGauss 发请求码 11（MD5_SHA256）。服务端计算 MD5 摘要时包含了存储串末尾的迭代次数后缀，客户端收不到这个后缀，无法通过认证（openGauss 自己的 libpq 也是按不含后缀计算）。补丁让这种情况报出明确的错误，提示改用 sha256。

已知的告警：`tokio-postgres/src/client.rs` 在 Windows 上有一个 unused import（上游代码，只在 unix 下使用），不影响构建。

## SQL Server 的影响行数、服务器消息和取消

tiberius 的结果流（`QueryStream`）只给出结果集和行：语句的影响行数（DONE / DONEINPROC 令牌中的行数）和服务器的 INFO 消息
（`PRINT`、级别 10 及以下的 `RAISERROR`）都被丢掉，而执行 SQL 时要显示它们（M6a 设计文档 3.4 节）。另外，结果集读到一半时取消，
连接会错位（见下表最后一项）。

| 库 | 上游版本 | 改动 |
|---|---|---|
| `tiberius` | 0.13.0 | `src/tds/stream/query.rs`：`QueryItem` 加 `Done(u64)`（设置了 DONE_COUNT 的 DONE、DONEINPROC）和 `Info(InfoMessage)`，`poll_next` 产出它们；`forward_to_metadata` 遇到这两种令牌时停下（不跳过）；其余对 `QueryItem` 的 `match` 补上新分支。新增 `InfoMessage` 结构，`src/result.rs` 导出它。`src/tds/codec/token/token_done.rs` 加 `has_count()`。`src/client/connection.rs`：`cancel_request` 发出取消后改为按包丢弃剩下的响应，直到某条响应以带 DONE_ATTN 的 DONE 令牌结束（新增 `drain_to_attention_ack`，返回 `()`）；原来按令牌读到当前响应末尾，结果集读到一半丢弃时确认会留在连接上，下一个请求得到空结果。原来的 `flush_done_attention`、`is_attention` 不再使用，标了 `allow(dead_code)` |

## 升级步骤

每个里程碑开头的依赖复查发现这些库有新版本时：

1. 从 crates.io 下载新版本（`cargo download` 或取 `$CARGO_HOME/registry/src/*/<库>-<版本>`），替换 `vendor/<库>/` 中除 `Cargo.toml` 改动以外的全部文件；
2. 按上表重新套用改动（`gauss.rs` 整个文件复制过去；其余是几行的插入或分支）；
3. 更新上表中的上游版本；
4. 运行 `CARGO_TARGET_DIR=target cargo test --manifest-path vendor/postgres-protocol/Cargo.toml --lib authentication`（构建目录用工作区的 `target`，不要在 vendor 下生成），再用 `DBLENS_TEST_OPENGAUSS_SHA_URL` 跑 PG 驱动的一致性测试；tiberius 用 `DBLENS_TEST_SQLSERVER_URL` 跑 SQL Server 驱动的一致性测试（其中多结果集、影响行数和 PRINT 消息的用例覆盖补丁）。
