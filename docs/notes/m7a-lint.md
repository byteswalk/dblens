# M7a 数据库设计规范检查

依据：设计文档 `docs/superpowers/specs/2026-10-30-m7-lint-design.md`，实施计划 `docs/superpowers/plans/2026-10-30-m7a-lint.md`。

## 结构

```text
crates/dblens-lint/                            规则引擎（只算，不连库）
  src/model.rs                                 Ruleset/Clause/RuleRef/Verdict/ClauseResult/Issue/Review、结论统计、校验
  src/engine.rs                                遍历启用的条款、按家族过滤规则、判定结论
  src/rules/{naming,table,column,index,object,consistency}.rs  各类规则，正反各一例单测
  src/rules/mod.rs                             规则注册表、参数取值（序号区分同编号规则）、规则编号和参数定义
  src/builtin.rs + builtin.yaml                内置三份规范（中英文写在 YAML）、差异的计算与应用（恢复默认=清差异）
  src/pinyin.rs                                拼音启发式（要求强音节，避开 time/name/date 这类英文词）
crates/dblens-dialect/src/ident.rs             公开 is_reserved_word、needs_quote
crates/dblens-service/src/lint/{mod,store,job,report}.rs + report.html.j2
  store.rs                                     lint_rulesets（内置存差异/自定义存全文）、lint_runs
  job.rs                                       规范增删改查、恢复默认、YAML 导入导出、起检查任务、人工结论、报告分发
  report.rs + report.html.j2                   HTML 报告（minijinja），标签走词条
crates/dblens-service/src/store.rs             本地库版本 5
crates/dblens-service/src/task.rs              TaskKind::Lint、TaskStatus.lint_run
crates/dblens-api/src/lint.rs                  /api/lint/*
crates/dblens-api/tests/lint.rs                模型来源的完整流程（规范库、检查、人工结论、报告）
web/src/features/lint/                         规范库、结果页、发起对话框、进度跟踪、纯逻辑+单测
web/src/features/{tree,model}/                 schema 右键发起检查
web/src/app/{menus,TitleBar,Shell}.tsx         文件菜单入口、挂对话框
```

## 接口

```text
GET/POST   /api/lint/rulesets                 列出、新建（自定义）
POST       /api/lint/rulesets/import          导入 YAML
GET/PUT/DELETE /api/lint/rulesets/{id}         读取、保存（内置存差异）、删除
POST       /api/lint/rulesets/{id}/reset       恢复默认（仅内置）
GET        /api/lint/rulesets/{id}/export      导出 YAML
GET        /api/lint/rules                     规则编号和参数定义
GET/POST   /api/lint/runs                      列出记录、发起检查（返回任务）
GET/DELETE /api/lint/runs/{id}                 读取、删除记录
PUT        /api/lint/runs/{id}/clauses/{clause}  填人工结论（仅无法判断/待人工确认）
GET        /api/lint/runs/{id}/report           导出报告（format=html）
```

## 与设计文档不一致、写实现时定的

- 规范差异按“条款编号 + 规则在条款里的序号”记（一条条款可挂多个同编号规则，如 name.prefix），不是按规则编号。
- 内置条款文字直接以中英文写进 `builtin.yaml`，不走词条表：用户复制、编辑后文字是普通字符串；也避开“未使用词条”检查。
- 需要查数据的规则（表数据量、低选择性）首期默认关闭，开启报“无法判断”，查询能力留到 M7c。
- 模型文件只有表：视图、序列、存储过程等对象规则对模型报“无法判断”。
- 报告文件名用记录编号（ASCII），避免中文对象名进 HTTP 头。

## 已验证

- `dblens-lint` 70 项单测（规则正反例、结论判定顺序、内置规范快照、差异往返）。
- `dblens-api` 3 项接口测试（规范库增改删导入导出、模型来源完整流程、HTML 报告）。
- 全量后端测试、i18n 词条检查（存在/双语/占位符/无冗余）、clippy 均通过。
- 前端 tsc、316 项前端测试、生产构建均通过。

## 待验证（真实运行）

- 五种数据库实库各跑一遍，核对“无法判断”的条款与设计一致（达梦只读）。
- 真实鼠标走完整流程并截图：开规范库、改参数、复制规范、发起检查、看结果、填人工结论、导出报告、恢复默认。
