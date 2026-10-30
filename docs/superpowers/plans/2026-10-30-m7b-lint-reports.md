# M7b 规范检查：例外管理与 Word、Excel 报告 实施计划

设计：`docs/superpowers/specs/2026-10-30-m7-lint-design.md` 第 8、9、10 节。

依赖审查：M7 开工时已做（`12c22ad`，2026-10-30），M7b 同日继续，不重复。

## 任务

1. **引擎**（`dblens-lint`）：`Ruleset.exceptions`、`ClauseResult.waived`/`clear`、`waive`/`unwaive`、`Summary.waived`；检查时应用例外；内置差异带上例外；校验例外。
2. **服务**（`dblens-service/src/lint`）：`waive_lint_issue`、`unwaive_lint_issue`。
3. **报告数据**：`report.rs` 抽出报告数据，HTML 补人工确认清单和豁免清单。
4. **Office 报告**：`document/docx.rs` 抽出 `assemble`，`document/xlsx.rs` 开放 `Sheet`；新增 `lint/report_office.rs` 写 Word、Excel、自查表。
5. **接口**（`dblens-api`）：`waivers` 两个路由；报告按格式给 Content-Type。
6. **界面**（`web/src/features/lint`）：问题右键豁免、已豁免清单和撤销、报告格式菜单、规范库的例外列表。报告改用下载而非 `window.open`。
7. **验证与文档**：全量测试、clippy、i18n、前端构建；真实鼠标走查；`docs/notes/m7b-lint-reports.md`，总设计进度。

## 验收

- 豁免后：问题数减少、结论重判、再次检查自动豁免、撤销后恢复。
- 四种格式都能导出，docx、xlsx 用解压检查内容；自查表列序和约束映射与设计一致。
- 工作区里 GBase 和 `TitleBar` 的未提交改动不进 M7b 的提交。
