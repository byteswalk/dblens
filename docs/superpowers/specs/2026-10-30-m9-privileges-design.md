# M9 数据库账号与权限管理

日期：2026-10-30。上级设计：总设计 `2026-10-22-dblens-design.md`（7.7 节执行策略）；M1c3 运维查询、M3 表设计器。里程碑编号暂定。

## 1. 目标与范围

在 DBLens 里管理**数据库自身**的账号和权限：先看清“谁有什么权限、权限从哪来、谁能访问这个对象”，再做修改。

包括：

- 查看用户、角色及其属性；
- 查看直接授权、角色成员关系、继承后的实际权限及来源路径；
- 从对象反查“谁能访问”；
- 建用户、删用户、改密、锁定和解锁，授予和回收权限及角色；
- 所有修改先预览 SQL，再确认执行；
- 密码不进入历史、日志和结果预览。

不包括：

- DBLens 自身的用户、RBAC、按连接授权（团队版，见第 11 节，本期不实现）；
- 软件许可和激活；
- 权限对比、导出授权脚本、把账号复制到另一环境（M9b 之后再议，模型按此预留）；
- 建议或自动收敛权限，比如“最小权限”分析；
- 行级安全、数据脱敏、审计策略的管理；
- 改密策略（Oracle profile、PG `password_encryption` 等）的配置；
- 云厂商控制台上的账号和 IAM 认证。

## 2. 分期

| 期 | 内容 | 依赖 |
|---|---|---|
| M9a | 只读：列表、属性、直接授权、实际权限、谁能访问 | 无，可与 M8a 并行 |
| M9b | 修改：账号操作、授予回收、SQL 预览、密码通道、历史脱敏 | M9a |
| M9c | GBase 8s 的 M9a 能力 | M8a 合并到 main；写能力另行决定 |

## 3. 数据库覆盖

遵循“协议而非产品”：模板按基础协议写，只覆盖确有差异的查询。

| 数据库 | 模式 | 主要来源 | 差异要点 |
|---|---|---|---|
| MySQL | | `mysql.user`、`role_edges`、`default_roles`、`procs_priv`，授权取 `information_schema` 的 `USER_`/`SCHEMA_`/`TABLE_`/`COLUMN_PRIVILEGES` | 主体是 `user@host`；host 可含通配符；partial revokes |
| MariaDB | | `mysql.user`（视图）、`global_priv`、`roles_mapping`，授权同 MySQL | 锁定状态在 JSON 里；角色 host 为空 |
| TiDB | | `mysql.user`、`role_edges`，授权取 `information_schema` | 没有 `procs_priv` |
| OceanBase CE | MySQL | `mysql.user`、`role_edges`，授权取 `information_schema` | 没有 `mysql.tables_priv`；只做 MySQL 模式 |
| PostgreSQL | | `pg_roles`、`pg_auth_members`、各目录表的 ACL | 用户即可登录的角色；PG16 起 `inherit_option` 按成员关系存放；属主、超级用户、PUBLIC |
| openGauss | A、PG | 同 PG，`aclexplode` 不用 LATERAL | `rolsystemadmin` 等角色属性 |
| 金仓 | Oracle、PG | 同 PG 模板，两种模式实测相同 | Oracle 模式也走 PG 目录，不套用 Oracle 视图 |
| Oracle | | `DBA_USERS`、`DBA_ROLES`、`DBA_ROLE_PRIVS`、`DBA_SYS_PRIVS`、`DBA_TAB_PRIVS`、`DBA_COL_PRIVS`；不足时退到 `ALL_`/`USER_` | `ANY` 系统权限；默认角色和启用状态；PUBLIC；公共用户和本地用户 |
| 达梦 | | DBA 视图；不足时退到 `ALL_`/`USER_` | 规划时未实连，M9a Task 5 修正；字段名与 Oracle 不同；旧版本缺视图 |
| SQL Server | | `sys.server_principals`、`server_role_members`、`server_permissions`、`database_principals`、`database_role_members`、`database_permissions` | 登录名和数据库用户分两级，按 sid 记映射；低权限查询不报错只是看不到别人；DENY 优先；固定角色隐含权限 |

上表除达梦外都在规划时实连核对过（高权限账号，只读）。GaussDB 等同协议产品不单独识别，走基础协议，不承诺。

## 4. 数据模型

放在 `dblens-dialect`，新模块 `privilege`。

```rust
pub struct Principal {
    pub id: String,              // 唯一标识，MySQL 为 user@host，SQL Server 含级别
    pub name: String,
    pub host: Option<String>,    // 只有 MySQL 家族有
    pub kind: PrincipalKind,     // User、Role、Login、Group
    pub level: PrincipalLevel,   // Server、Database
    pub can_login: Option<bool>,
    pub locked: Option<bool>,
    pub expires_at: Option<String>,
    pub builtin: bool,           // 系统内置，界面默认不列
    pub attrs: Vec<(String, String)>, // 方言特有属性，原样展示
}

pub struct Membership {
    pub member: String,          // Principal.id
    pub role: String,
    pub admin_option: bool,
    pub inherit: Option<bool>,   // PG 系
    pub default_role: Option<bool>, // Oracle、MySQL
}

pub struct PrivilegeGrant {
    pub grantee: String,
    pub scope: GrantScope,       // Global、Database、Schema、Object、Column
    pub target: GrantTarget,     // 库、模式、对象类型、对象名、列名，缺省即不适用
    pub privilege: String,       // 保留各库原文
    pub effect: GrantEffect,     // Grant、Deny、Implicit（属主、超级用户等隐含权限，不是授权记录）
    pub grantable: bool,
    pub grantor: Option<String>,
}
```

- 权限名不映射成统一枚举。不同库的同名权限含义不同，硬映射会误导。
- 现有 `MetaKey::Grants` 和 `Grant` 结构只有表级，留给原有功能，不改。
- 新模板按运维查询的方式放在 `sql/privilege/<方言>.sql`，用现有模板机制，变体只覆盖有差异的段落。
- 模板段落：`caps`（可选，查看范围和 partial revokes）、`principals`、`memberships`、`grants_of`（一个主体，服务层逐个读取）、`grants_on`（一个对象，含覆盖它的上级范围）；段名带 `@limited` 的是低权限退路。模板直接输出统一列名。

## 5. 读取流程

账号和授权可能很多，不做全量快照：

1. 打开标签页：读 `principals` 和 `memberships`；
2. 选中主体：算出它经由角色可达的主体集合，读这些主体的 `grants_of`；
3. 反查对象：读 `grants_on`，再用成员关系反向展开到用户。

不缓存到本地，打开和手动刷新时才读，修改成功后重读当前主体。

### 权限不足

低权限账号读不到 DBA 视图时，按每个段落依次退到 `ALL_`、`USER_` 或 `information_schema`，并返回：

```rust
pub enum Coverage {
    Complete,
    Partial { missing: Vec<Msg> }, // 比如“系统权限”“其他用户的授权”
}
```

界面在顶部显示范围不完整。没有查询权限和“确实没有授权”必须能区分，读不到的段落不显示为空结果。

## 6. 实际权限

计算放在 `dblens-dialect`，是纯函数，便于离线测试。输入主体、成员关系和授权，输出：

```rust
pub struct EffectivePrivilege {
    pub grant: PrivilegeGrant,
    pub via: Vec<String>,        // 路径，比如 [u1, role_a, role_b]
    pub state: EffectiveState,   // Allowed、Denied、Conditional
    pub notes: Vec<PrivNote>,    // 枚举，文字由前端词条给出
}
```

成员关系有环时（部分库允许）停止展开，不报错。同一权限有多条路径时全部保留。

下列情况不算成确定结论，用 `Conditional` 或 `notes` 标明：

| 情况 | 处理 |
|---|---|
| MySQL `user@host` 通配符 | 列出匹配的账号定义，不判断实际命中哪一条 |
| MySQL partial revokes | 标注“存在部分回收”，结果标为 `Conditional` |
| MySQL、Oracle 角色未默认启用 | 标注“需在会话中启用” |
| PG 系 `NOINHERIT` | 路径保留，标注“需 SET ROLE” |
| SQL Server DENY | 同范围及上级的 DENY 优先，结果 `Denied`；sysadmin 不受 DENY 限制 |
| SQL Server 固定角色、Oracle DBA 等内置角色 | 显示角色，不展开隐含权限，除非字典能查到 |
| Oracle `ANY` 系统权限 | 算入“谁能访问”，标注来源是系统权限 |
| 超级用户、对象属主 | 算入“谁能访问”，标注原因，不伪造授权记录 |
| PUBLIC | 单独列出，标注“所有用户” |

界面措辞用“按字典计算”，不写“确定有权限”。

## 7. 接口

服务层新增 `privilege` 模块，读操作走元数据连接，不进入执行历史。接口路径在实施计划里按现有 REST 风格定，语义如下：

| 操作 | 期 |
|---|---|
| 列出主体和成员关系 | M9a |
| 读取某主体的授权和实际权限 | M9a |
| 反查某对象的访问者 | M9a |
| 生成变更 SQL（只返回预览） | M9b |
| 执行已确认的变更 | M9b |

错误信息用 `Msg`，走现有 i18n。

## 8. 界面

沿用现有布局和黑白基调，不新增活动栏图标，不用彩色标签和大按钮。

入口：

- 连接节点右键“用户与权限”，打开标签页；
- 表、视图等对象右键“谁能访问”，打开同一标签页并定位到该对象。

同一连接只开一个此类标签页。

标签页布局：

- 左侧：主体列表，可搜索，可按用户和角色筛选。SQL Server 按服务器级和数据库级分组。
- 右侧三个视图：**属性**、**直接授权**、**实际权限**。实际权限每行显示路径，可按范围、对象、权限名筛选。
- 反查模式：右侧列出用户、权限、来源路径；点击用户跳到该用户。

M9b 在此基础上加操作入口，统一弹出 SQL 预览对话框。

## 9. M9b 修改

### 9.1 生成 SQL

`dblens-dialect` 按方言生成语句，标识符用现有引用工具，全部加快照测试。预览里显示将执行的完整语句，密码显示为 `***`。一次操作含多条语句时不保证原子：逐条执行，失败即停，并显示已成功的语句。

### 9.2 密码通道

现状：执行历史没有脱敏，SQL 原文会写入本地存储。因此：

- 专用执行路径不调用 `history.record` 原文，历史写入脱敏后的语句；
- 密码只存在于请求体和发给数据库的语句里，不写日志，不进错误信息；
- 数据库报错如回显语句，先脱敏再返回；
- 前端用专用密码框，关闭对话框即清空，不写入 localStorage 或草稿；
- 随机密码在本地生成，只显示一次。

用户在编辑器里手写的语句也有同样问题。历史记录入口增加尽力脱敏：`CREATE`/`ALTER` 的 `USER`、`ROLE`、`LOGIN` 语句和 `SET PASSWORD` 里的密码字面量替换为 `***`。解析失败时，对这几类语句只记录首关键字和对象名，宁可少记。

### 9.3 策略

现状：`DefaultPolicy` 只在生产连接上确认 `DROP`，`CREATE USER`、`ALTER USER`、`GRANT`、`REVOKE` 都直接放行。

- M9b 的界面操作一律先预览再确认，不分环境；
- 生产连接上再加一层：确认框显示连接名和环境，高风险操作要输入主体名，包括删用户、回收 DBA 或 sysadmin 这类管理员角色、授予管理员角色；
- 专用路径同样调用 `Policy`，保证团队版可接管；
- 在 `DefaultPolicy` 里增加对生产连接上 `GRANT`、`REVOKE`、`CREATE USER`、`ALTER USER` 的确认，编辑器手写时也生效。这是行为变化，写入发布说明。

### 9.4 操作范围

| 操作 | 说明 |
|---|---|
| 建用户、建角色 | 只含名称、密码和各库通用属性，其余用 SQL 自行完成 |
| 删用户、删角色 | 删除前显示持有对象和授权情况；不自动 CASCADE，由用户勾选 |
| 改密、锁定、解锁 | 方言不支持的操作不显示 |
| 授予和回收 | 对象、模式、库、全局；角色；可转授选项 |

修改当前连接自己的账号时，提示可能导致现有连接失效。

## 10. 错误处理

| 情况 | 行为 |
|---|---|
| 某段落无权限 | 退级读取，`Coverage::Partial` |
| 版本缺视图或字段 | 该段落按缺失处理，不让整页失败 |
| 查询超时 | 显示错误和重试，其余段落保留 |
| 主体已被删除 | 刷新列表并提示 |
| 修改中途失败 | 显示已成功语句和错误，重读当前主体 |
| 方言不支持的能力 | 不显示入口 |

## 11. 团队版边界（只规划）

| 项 | 归属 |
|---|---|
| 数据库账号和权限（本设计） | 个人版 |
| DBLens 用户、登录、RBAC | 团队版，需要服务端多用户鉴权 |
| 按连接和对象授权、时间窗 | 团队版，通过 `Policy` 接口实现，思路来自 dbsecurity 的 `UserRoleDbPermissionDialog`、`USERDENYTIME` |
| 凭据托管、审计、SQL 审批 | 团队版 |

团队版另开设计，前提是调研报告列出的付费意愿假设得到验证。M9 不为它添加代码，只保持修改操作都经过 `Policy`。

## 12. 测试

- **离线：** 模板渲染、行归一化、实际权限计算、SQL 生成和脱敏，用单元测试和快照覆盖。实际权限重点测环、DENY、多路径、NOINHERIT。
- **脱敏：** 用哨兵密码跑完整链路，断言历史、日志、错误信息、预览里都不出现。
- **实连：** 在 VM 实例上创建 `dblens_priv_*` 夹具账号和角色，覆盖第 3 节全部数据库及模式，用高权限和低权限两种连接各跑一遍，结束时清理。全部串行，`--test-threads=1`。SQL Server 同时测 2022 和 2025。
- **用户的真实达梦实例：** 只做只读测试。M9b 的写操作在 VM 的 DM8 上验证。
- **界面：** 用真实鼠标操作和截图验证，含暗色主题和中英文。

## 13. 验收

M9a：

- 第 3 节每种数据库都能列出主体、成员关系和授权，夹具账号的实际权限与预期一致；
- 三层角色继承能显示完整路径；
- 对象反查能列出直接授权、角色继承、PUBLIC 和属主；
- 低权限连接不报整页错误，并提示范围不完整；
- 第 6 节列出的情况都有标注。

M9b：

- 每项操作在支持它的数据库上实连通过；
- 哨兵密码不出现在任何落盘内容里；
- 生产连接上的高风险操作必须输入主体名才能执行；
- 编辑器手写的建用户语句，历史里看不到密码。
