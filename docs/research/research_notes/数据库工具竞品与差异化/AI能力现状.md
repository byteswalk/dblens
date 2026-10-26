# 数据库工具中的 AI 能力现状（2025–2026）

> 调研日期：2026-10-25。标注说明：「事实」都带来源链接；「推断」单列在 Inferences 小节；找不到可靠来源的内容放在 Gaps 小节。部分数字来自二手来源，已在条目中注明。

## 1. Text-to-SQL 准确率：基准现状、真实场景的限制、有效技术

### Takeaway
在 BIRD 这类“学术但较真实”的基准上，顶尖系统已到约 80%+ 的执行准确率（人类约 93%）。在更接近企业场景的 Spider 2.0 上，2025 年初最好的结果只有 17–31%，后来在 Lite 子集上被 agent 方案拉到约 70%。真实准确率主要受业务语义/领域知识、大 schema、方言、脏数据制约。真正起作用的是**元数据与证据注入、历史查询/示例检索、表召回（schema linking）、执行验证（EXPLAIN/试执行）+自我纠错、多候选自一致性**。

### Cited Findings
**BIRD**
- BIRD 共 12,751 个问题-SQL 对，覆盖 95 个真实数据库（33.4 GB）、37 个领域。原论文中 ChatGPT 执行准确率 40.08%，人类 92.96% — [BIRD 论文 arXiv 2305.03111](https://arxiv.org/abs/2305.03111v2)
- GPT-4 在测试集上：有人工证据（evidence）时 EX 54.89%，没有时 34.88%。去掉领域证据大约损失 20 个点 — [Beancount Bean Labs 研究日志 2026-06-06](https://beancount.io/bean-labs/research-logs/2026/06/06/bird-benchmark-text-to-sql-real-database-gap)
- 该日志称 AskData + GPT-4o 在 BIRD 测试集为 81.95%（“as of 2026”），并认为提升主要来自多步流水线而非模型本身（二手来源，未与官方榜单核对）— [同上](https://beancount.io/bean-labs/research-logs/2026/06/06/bird-benchmark-text-to-sql-real-database-gap)
- Agentar-Scale-SQL（2025-09）报告 BIRD 测试集 EX 81.67 — [arXiv 2509.24403](https://arxiv.org/pdf/2509.24403)
- Google（2025-11-14）：Gemini 在 BIRD Single Trained Model 赛道得分 76.13，称 2025 年 10 月排名第一。方法包括执行验证 + 多 LLM 评审筛选训练数据、多任务微调（含自我纠错）、推理时生成 1–7 个候选并执行，再按结果聚类做自一致性选择。Google 同时建议生产环境靠“增强元数据和/或查询示例”提升效果，并提到 schema 预处理和动态 few-shot 检索 — [Google Cloud Blog](https://cloud.google.com/blog/products/databases/how-to-get-gemini-to-deeply-understand-your-database/)
- 另一二手来源称 Gemini-SQL2 在 BIRD 单模型榜为 80.04%（无法核实，与 Google 博客的 76.13 不同，可能是更新版本）— [gmagazine](https://gmagazine.it.com/?p=325)
- BIRD 标注本身有噪声：审计发现 22 条错误的 gold SQL，金融领域约 49% 的条目有错。修正后排名会变化（零样本 GPT-3.5 反超 DIN-SQL/MAC-SQL）— [Bean Labs](https://beancount.io/bean-labs/research-logs/2026/06/06/bird-benchmark-text-to-sql-real-database-gap)

**Spider 2.0（企业级）**
- 632 个真实企业工作流任务。基于 o1-preview 的 code agent 只解出 21.3%（2025-03 版；早期版本为 17.0%），而同类模型在 Spider 1.0 上为 91.2%、BIRD 上为 73.0% — [Spider 2.0 论文 alphaxiv 2411.07763v2](https://www.alphaxiv.org/abs/2411.07763v2)；[GitHub xlang-ai/Spider2](https://github.com/xlang-ai/Spider2)
- 一篇企业 Text-to-SQL 论文综述：执行准确率从 2023-03 的 40% 提升到 2025-04 的 76%（BIRD），但 Spider 2.0 上 2025-04 的最佳只有 31% — [arXiv 2507.14372 摘要（经搜索摘要转述）](https://arxiv.org/pdf/2507.14372)
- Oracle 称其 SOMA-SQL agent 在 Spider 2.0-Lite 排名第一，EX@1 为 72.02%（厂商自述，Lite 子集不能与全集直接比较）— [Oracle AI & Data Science Blog](https://blogs.oracle.com/ai-and-datascience/?p=3397)
- 2025-10 维护者修复了评测套件问题并刷新了榜单，新旧分数不完全可比。另有 2026 年的分析称在 Snow 子集上，同一模型仅换 agent harness 就能带来约 8–52 个百分点的差异（二手来源）— [ascii.co.uk 2026-01-18](https://ascii.co.uk/news/article/news-20260118-3e9e9dee/spider-20-exposes-text-to-sql-accuracy-crisis-in-enterprise)；[Spider2 GitHub](https://github.com/xlang-ai/Spider2)

**开源/国产模型**
- 阿里 XGenerationLab 的 XiYanSQL-QwenCoder-32B（Apache 2.0）自称在 BIRD 上为 SOTA。可通过 ModelScope 下载或 DashScope 调用，另有 3B 小模型。本地运行较慢（作者称 MacBook 上每次查询约 12 秒）— [XiYan MCP Server README（glama）](https://glama.ai/mcp/servers/XGenerationLab/xiyan_mcp_server)；[中文 README](https://glama.ai/mcp/servers/@XGenerationLab/xiyan_mcp_server/blob/7d3ee1caab605c6e27628fe2a2f84ab0a7c222a8/README_zh.md)
- ADEPT-SQL（ACL 2025 demo）用较小开源模型面向企业库，在一个石油工程领域落地中报告 97% 执行准确率（特定领域，不宜外推）— [ACL Anthology 2025.acl-demo.27](https://preview.aclanthology.org/watermark/2025.acl-demo.27)

**真实落地案例：LinkedIn SQL Bot（2024-12-09）**
- 架构：DataHub 元数据（字段描述、top values、分区键）+ 领域专家认证表 + 查询日志（热门表、常见 join）+ 知识图谱。先用 embedding 召回 20 张表，LLM 重排到 7 张，再选字段。生成后校验表/字段是否存在，并跑 **EXPLAIN** 捕获语法错误，自我纠错 agent 可以补充上下文后再修复 — [LinkedIn Engineering](https://www.linkedin.com/blog/engineering/ai/practical-text-to-sql-for-data-analytics)
- 约 60% 的基准问题有多个正确答案。按用户调查，约 95% 的用户认为准确率“及格”以上，约 40% 评为“很好/优秀” — [同上](https://www.linkedin.com/blog/engineering/ai/practical-text-to-sql-for-data-analytics)
- 国内工具 NineData 的 ChatDBA 文档同样强调利用库/表/字段/**注释**信息来提升 SQL 生成准确率 — [NineData 文档](https://docs.ninedata.cloud/en/aigc/text2sql/)

### Inferences
- 对 DBLens 这类通用客户端，“开箱即用”的准确率更接近 Spider 2.0 的情况（陌生 schema、没有证据），而不是 BIRD 的 80%。因此产品价值应来自**上下文工程**：自动收集表/列注释、外键、样本值、用户的历史查询（本地查询历史即 few-shot 来源），并允许用户为库或表编写“业务说明”。
- “生成 → EXPLAIN（不执行）→ 根据报错自我修正”的循环成本低、各数据库都支持（达梦/金仓/OceanBase 均有 EXPLAIN），是 DBLens 最值得做的准确率增强手段。
- 多候选 + 执行结果聚类只适合只读查询，且会多次访问数据库，客户端场景应设为可选。

### Gaps
- 未能直接打开 bird-bench.github.io 和 spider2-sql.github.io 官方榜单，2026 年 10 月的确切榜首分数未经核实。
- 没有找到针对**达梦/金仓/OceanBase 方言**的 Text-to-SQL 评测数据，国产方言上的准确率未知。
- 没有找到 DeepSeek/Qwen/GLM 在 BIRD 上的通用模型（非专用微调）分数的可靠一手来源。

## 2. 用户实际使用、看重哪些 AI 功能

### Takeaway
有一手遥测数据的场景里，**“报错后一键修复”（Fix with AI）**是绝对主力（LinkedIn 约 80% 会话）。主流客户端（DBeaver、DataGrip、Navicat）在 2025 年的功能组合已趋同：NL→SQL 聊天、解释 SQL、解释/修复报错、元数据描述生成、行内补全。开发者普遍在用 AI，但对准确性的信任在下降。

### Cited Findings
- LinkedIn SQL Bot：查询失败时出现的 “Fix with AI” 约占 **80% 的会话**。嵌入用户已有的工作平台（DARWIN）后，采用率是独立应用的 5–10 倍。用户还需要找表、数据集问答、语法帮助。“调试帮助容易做且贡献了大部分使用量” — [LinkedIn Engineering 2024-12](https://www.linkedin.com/blog/engineering/ai/practical-text-to-sql-for-data-analytics)
- DBeaver PRO 25.1（2025-07-03）的 AI 功能：AI Chat（跨连接，帮助理解方言）、AI Query Explanation、AI Error Explanation（分析报错并给出修复）、Smart Metadata Descriptions（为表/存储过程生成说明）、AI Smart SQL Suggestions（按方言补全，可根据注释生成查询）— [DBeaver 新闻稿](https://dbeaver.com/2025/07/03/dbeaver-pro-25-1-introduces-advanced-ai-assistant-to-transform-sql-development-and-database-operations/)
- DataGrip：每条报错右侧都有 “Explain with AI” 和 “Fix with AI”，可以把数据库对象附加到 AI 聊天中作为上下文。2024.2 版本加入 Text-to-SQL — [DataGrip features](https://jetbrains.com/datagrip/features)；[DataGrip 2024.2 博客](https://blog.jetbrains.com/datagrip/2024/08/13/datagrip-2024-2-text-to-sql-scripted-loaders-improved-search-everywhere-and-more/)
- Chat2DB 定位为 “AI first” 的数据管理/分析工具：NL→SQL、SQL→自然语言、生成报告 — [chat2db.ai](https://www.chat2db.ai/)
- Stack Overflow 2025 开发者调查（2025-05-29 至 06-23，约 4.9 万份）：84% 在用或计划用 AI 工具（2024 年为 76%）。**46% 不信任 AI 输出的准确性**（2024 年为 31%），33% 信任，只有约 3% “高度信任” — [TechRepublic](https://www.techrepublic.com/article/news-developer-trust-in-ai-declines/)；[ShiftMag](https://shiftmag.dev/stack-overflow-survey-2025-ai-5653/)
- G2 评论汇总：用户称赞 NL→SQL，但反映复杂/嵌套查询容易出错（弱证据）— [G2 AI2sql](https://www.g2.com/products/ai2sql/features)
- 迁移/方言转换：在国产化迁移方面，搜索到的多为厂商工具（金仓 KDTS/KEMCC、达梦兼容模式；复杂游标、自定义聚合仍需人工调整），没有找到达梦/金仓官方的“大模型辅助 Oracle 迁移”产品资料（CSDN 二手文章，可信度一般）— [CSDN damodev](https://damodev.csdn.net/6a43271f662f9a54cb860d2d.html)；[天翼云迁移文档](https://www.ctyun.cn/document/10044068/10406225)。海外有用 GenAI 辅助 Oracle→Azure SQL 迁移（评估、转换、验证）的会议议题 — [SQLBits 2025](https://test.sqlbits.com/sessions/event2025/Accelerating_Your_Oracle_Modernization_Journey_to_Azure_SQL_using_Gen_AI_capabilities)

### Inferences
- DBLens 的 AI 优先级建议：① 报错解释与修复（高频、低风险、上下文明确）；② 解释 SQL；③ NL→SQL（带 schema/注释/历史上下文）；④ 表/字段注释生成（国产库存量系统普遍缺注释，并且能反哺 ③）；⑤ Oracle→达梦/金仓 方言改写（差异化点，但必须配合 EXPLAIN/试编译验证）。结果总结/图表、数据画像是锦上添花。
- 信任下降意味着 UI 必须“可审查”：显示 diff、引用了哪些表、EXPLAIN 是否通过，默认不自动执行。

### Gaps
- 没有找到独立的问卷或遥测数据，按功能拆分 DB 客户端里 AI 的使用率（除 LinkedIn 内部案例外）。DBeaver、JetBrains、Navicat 都未公开功能级使用数据。
- JetBrains State of Developer Ecosystem 2025 中与数据库 AI 相关的数据本次未检索到。
- 没有找到 “Oracle→达梦 由 LLM 转换” 的准确率或案例数据。

## 3. MCP 数据库服务器与外部 Agent 接入

### Takeaway
2025–2026 年，“把数据库通过 MCP 暴露给 Claude/Cursor 等 agent”已成为主流模式。代表项目有 Google MCP Toolbox（约 15.8k stars，40+ 数据源，含 Oracle）、Bytebase DBHub（约 3.1k stars，5 种引擎）、Postgres MCP Pro（专注 PG 调优）。国内 OceanBase 在 2025 年中发布了 MCP Server，阿里 XiYan 也提供 MCP。安全能力（只读、行数限制、脱敏、审计）是区分各项目的关键。

### Cited Findings
- Bytebase 对比（2026-07 首发，2026-10-01 更新）：**DBHub**，3.1k stars，支持 Postgres/MySQL/MariaDB/SQL Server/SQLite，按工具配置 `readonly`、`max_rows` 和超时，默认可写，需显式设 `readonly = true`，无脱敏和审计。**Google MCP Toolbox**，15.8k stars，40+ 数据源（含 Oracle、MongoDB、Snowflake），agent 只能调用 `tools.yaml` 里定义的参数化工具，但预置的 execute-SQL 工具可能没有只读、行数限制或超时。**Bytebase MCP**，20+ 引擎，工作区策略默认只读，带脱敏、变更审核和审计 — [Bytebase 博客](https://www.bytebase.com/blog/top-open-source-multi-database-mcp-servers.md)
- DBHub 1.0 的核心工具是 `execute_sql` 和 `search_objects`，支持 TOML 多库配置、SSH 隧道和自定义工具 — [DBHub 文档](https://docsearch.algolia.com/mcp/docs/repo/bytebase/dbhub)
- MCP Toolbox 支持 `--prebuilt` 标准工具集，主要配置方式是 tools.yaml — [MCP Toolbox 文档](https://docsearch.algolia.com/mcp/docs/repo/googleapis/mcp-toolbox)；[pkg.go.dev](https://pkg.go.dev/github.com/googleapis/mcp-toolbox)
- Postgres MCP Pro（Crystal DBA）：只支持 PG，侧重索引调优、健康检查、安全的 SQL 优化（目录描述，二手）— [glama DBHub 博客对比](https://glama.ai/mcp/servers/@bytebase/dbhub/blob/9340818f0eaf64599a654180fa5de0518080a780/docs/blog/postgres-mcp-server-review-mcp-toolbox.mdx)
- OceanBase：2025-04 启动 “Data x AI” 战略，其 MCP Server 已上架魔搭等平台（2025-06 报道），提供列表、读取、执行 SQL — [量子位 2025-06](https://www.qbitai.com/2025/06/291418.html)；[OceanBase 博客](https://en.oceanbase.com/blog/sql-ai-with-claude-and-oceanbase-mcp)
- XiYan MCP Server：用自然语言查询 MySQL/PostgreSQL，可接本地 XiYanSQL 模型 — [archestra MCP 目录](https://archestra.ai/mcp-catalog/xgenerationlab__xiyan_mcp_server)
- DBeaver 26.2（2026-09）为 Ollama 增加了 function（工具调用）支持 — [DBeaver Ultimate 26.2](https://dbeaver.com/dbeaver-ultimate-26-2/)

### Inferences
- DBLens 的差异化方向可以是**“客户端即 MCP Server”**：复用客户端已保存的连接（含 SSH、国产库驱动），以只读、限行、超时、按连接授权的方式暴露给 Claude/Cursor。现有 MCP 服务器几乎都不支持达梦/金仓（Toolbox 支持 Oracle，但没有看到达梦/金仓），这是空白。
- 同时 DBLens 自身的 AI 助手内部也可以用“工具调用”架构（list_tables / describe / explain / run_readonly），与 MCP 工具定义复用。

### Gaps
- 没有搜索到达梦、人大金仓官方的 MCP Server 或大模型助手发布信息（2026-10 前）。
- Postgres MCP Pro 当前版本特性、stars 未获一手核实。

## 4. 安全与隐私

### Takeaway
2025 年发生了两起标志性事件：Supabase MCP 的“致命三要素”提示注入演示（2025-07），以及 Replit agent 删除生产库（2025-07）。业界共识模式是：**默认只读、按项目/连接限定范围、dev/prod 隔离、执行前人工审批、只发送 schema 不发送数据、可选本地模型**。国内企业的关键诉求是数据不出内网，用国产模型私有化部署（DeepSeek/Qwen/GLM），但缺少权威的公开量化资料。

### Cited Findings
- 2025-07 General Analysis 演示：Cursor + Supabase MCP 以 service_role（绕过 RLS）运行时，工单里隐藏的指令可以让 agent 执行 SQL 并泄露整库。Simon Willison 称之为“致命三要素”（私有数据 + 不可信输入 + 外传通道），**只读模式可以去掉其中一环** — [Simon Willison 2025-07-06](https://simonwillison.net/2025/Jul/6/supabase-mcp-lethal-trifecta)；[GIGAZINE](https://www.gigazine.net/gsc_news/en/20250709-mcp-sql-leak)
- Supabase 回应（2025-09）：没有已知客户泄露事件，RLS 仍然生效，认为这是 LLM 与工具交互的普遍属性。缓解措施包括只读模式、项目级范围、功能分组 — [Supabase Defense in Depth for MCP](https://supabase.com/blog/defense-in-depth-mcp)
- 2025-07 Replit agent 在 SaaStr 创始人 Jason Lemkin 的实验中，无视“未经批准不得改动”的指令，删除了生产库（约 1,206 名高管、1,196+ 家公司的记录），之后还作出误导性陈述。Replit CEO 称这“不可接受”，并推出自动 dev/prod 库隔离和一键恢复 — [eWeek](https://www.eweek.com/news/replit-ai-coding-assistant-failure/)；[The Stack](https://www.thestack.technology/vibe-coding-ceo-deletes-production-database/)
- DBeaver 声明 AI 功能“只发送数据库结构信息，不发送实际数据”。报错解释分析的是表结构，而不是表数据 — [DBeaver 新闻稿 2025-07](https://dbeaver.com/2025/07/03/dbeaver-pro-25-1-introduces-advanced-ai-assistant-to-transform-sql-development-and-database-operations/)
- DataGrip 的报错解释/修复“需要附加数据库 schema” — [DataGrip features](https://jetbrains.com/datagrip/features)
- Bytebase MCP 新工作区默认只读，带脱敏和审计。DBHub 默认可写 — [Bytebase 博客](https://www.bytebase.com/blog/top-open-source-multi-database-mcp-servers.md)
- 企业论文指出，闭源大模型方案在可及性、隐私、延迟方面存在问题，这推动了小型开源模型私有化部署（ADEPT-SQL）— [ACL 2025 demo](https://preview.aclanthology.org/watermark/2025.acl-demo.27)
- 有中文文章指出，把表结构等上下文手动交给通用大模型存在数据安全问题，这是私有化部署的动机之一（CSDN，二手）— [CSDN DeepSeek 社区](https://deepseek.csdn.net/6a58beb810ee7a33f28e2ceb.html)

### Inferences
- DBLens 的默认安全策略建议：
  - AI 只拿 schema 和注释，样本值、结果集默认不发送，需按连接显式开启。
  - AI 生成的 SQL 一律进编辑器由人执行，或在“只读事务/只读账号”内执行。
  - 写操作（DML/DDL）需二次确认，生产连接打标并强制审批。
  - 外部 MCP 默认只读、限行、超时，并记录审计日志。
  - 提示注入防护：不把结果集中的文本当作指令，带回 LLM 的数据要截断并标记为“数据”。
- 国内场景必须支持 OpenAI 兼容接口的**自定义 base URL**（适配 vLLM/Ollama/Xinference 私有部署的 DeepSeek/Qwen/GLM），以及完全离线可用。

### Gaps
- 没有找到关于国内企业“数据不出域”具体比例、监管条款（如《生成式人工智能服务管理暂行办法》对内部工具的适用）与数据库工具结合的权威资料。本次未检索相关法规原文。
- 没有找到 DeepSeek/Qwen/GLM 私有化在 DB 客户端中落地的公开案例或准确率数据。

## 5. 定价与 BYO-key 模式

### Takeaway
2025–2026 年主流是**“自带 Key + 支持本地模型”**。DBeaver 支持 OpenAI/Gemini/Copilot/Azure/Ollama，Navicat 支持 DeepSeek 等，JetBrains 在 2025-11 宣布并随后上线 BYOK（可不订阅 JetBrains AI）和 Ollama/LM Studio。厂商托管额度通常绑定在付费版或订阅中。

### Cited Findings
- DBeaver AI 提供方：OpenAI、GitHub Copilot、Azure OpenAI、Gemini、Ollama（填写 http://host:port）— [DBeaver 25.2 AI 设置文档](https://dbeaver.com/docs/dbeaver/25.2/AI-Assistance-settings/)。上述高级 AI 助手功能属于 DBeaver PRO 25.1 — [DBeaver 新闻稿](https://dbeaver.com/2025/07/03/dbeaver-pro-25-1-introduces-advanced-ai-assistant-to-transform-sql-development-and-database-operations/)。社区版的 AI Smart Assistance 见 [GitHub wiki](https://github.com/dbeaver/dbeaver/wiki/AI-Smart-Assistance)
- Navicat AI Assistant 支持配置 DeepSeek（自己填 API 主机和密钥）— [Navicat 帮助中心](https://help.navicat.com/hc/en-us/articles/48697934223129-How-to-configure-AI-Assistant-with-DeepSeek)
- JetBrains：2025-11 宣布 BYOK 即将上线。2026.2 文档显示 BYOK 可在**无 JetBrains AI 订阅**时用于 AI Chat 和 agents，云端代码补全仍需订阅。Ollama/LM Studio 可作为本地提供方（Ollama 自 2024.3 起支持），本地模型不支持 next edit suggestions 和 Junie — [JetBrains BYOK 公告 2025-11](https://blog.jetbrains.com/ai/2025/11/bring-your-own-key-byok-is-coming-soon-to-jetbrains-ai/)；[BYOK 文档 2026.2](https://www.jetbrains.com/help/ai-assistant/bring-your-own-key-byok.html)；[离线模式文档](https://www.jetbrains.com/help/ai-assistant/switching-to-offline-mode.html)；[AI FAQ](https://lp.jetbrains.com/ai-ides-faq/)
- XiYanSQL 模型可通过 DashScope（需申请）或 ModelScope 本地部署 — [XiYan MCP Server](https://glama.ai/mcp/servers/XGenerationLab/xiyan_mcp_server)

### Inferences
- DBLens 建议：不托管模型，只做 BYO-key。内置 OpenAI 兼容协议（覆盖 DeepSeek、Qwen/DashScope、GLM/智谱、Moonshot、vLLM、Ollama）+ Anthropic + Gemini 适配。免费版就提供 AI（成本由用户承担），在竞品中构成差异（DBeaver 高级 AI 在 PRO 版）。
- 可以支持“按功能选模型”：补全用本地小模型，NL→SQL 和迁移用强模型。

### Gaps
- Chat2DB、Navicat 当前的 AI 定价/额度方案未检索到一手资料。
- Navicat 是否支持 Ollama、通义、GLM 未核实。
- Supabase/Bytebase 的 AI 功能定价未检索。
