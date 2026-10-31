# M7b 规范检查：例外管理与 Word、Excel 报告

依据：设计文档 `docs/superpowers/specs/2026-10-30-m7-lint-design.md`，实施计划 `docs/superpowers/plans/2026-10-30-m7b-lint-reports.md`。

## 结构

```text
crates/dblens-lint/src/model.rs                Exception、WaivedIssue、CleanVerdict、ClauseResult::waive/unwaive/settle
crates/dblens-lint/src/engine.rs               算出没有问题时的结论，再应用例外
crates/dblens-lint/src/builtin.rs              Overrides.exceptions：内置规范的例外存在差异里
crates/dblens-service/src/document/docx.rs     抽出 assemble（正文由调用方写）；没有目录时不要求更新字段
crates/dblens-service/src/document/xlsx.rs     开放 Sheet
crates/dblens-service/src/lint/report.rs       Report、Row：各格式共用的报告数据；HTML
crates/dblens-service/src/lint/report_office.rs  Word、Excel、自查表
crates/dblens-service/src/lint/job.rs          waive_lint_issue、unwaive_lint_issue、ReportFormat
crates/dblens-api/src/lint.rs                  waivers 路由，报告按格式给 Content-Type
web/src/features/lint/RunView.tsx              右键豁免、已豁免清单和撤销、报告格式菜单
web/src/features/lint/LibraryView.tsx          例外列表（删除后保存）
```

## 接口

```text
POST   /api/lint/runs/{id}/clauses/{clause}/waivers           {object, reason}，返回更新后的记录
DELETE /api/lint/runs/{id}/clauses/{clause}/waivers?object=   返回更新后的记录
GET    /api/lint/runs/{id}/report?format=html|docx|xlsx|checklist&reviewer=
```

## 与设计文档不一致、写实现时定的

- `ClauseResult.clean` 存“没有问题时的结论和原因”。豁免掉全部问题后回到它，所以条款可能变成通过、无法判断或待人工确认。M7a 的旧记录没有这个字段，按通过算。
- 问题恢复时清掉人工结论：人工结论只属于无法判断、待人工确认的条款。
- 例外不校验条款是否存在：内置规范换版本后，旧例外留着无害。
- 补上了 M7a 漏掉的 HTML 人工确认清单（词条已有，模板没用）。
- 报告和规范导出从 `window.open` 改为下载。
- Word 报告用内置样式来源，不提供模板选择。
- 自查表的说明列把原因、问题、豁免写在一格，用分号隔开；单元格超长时按 M4c 的做法截断，明细以 Excel 报告为准。

## 已验证

- `dblens-lint` 78 项单测，`dblens-service` 4 项报告测试，`dblens-api` 3 项接口测试（含豁免、再次检查、四种格式、撤销）。
- 真实鼠标（模型来源，`dblens-server` 加临时数据目录）：发起检查、右键豁免、原因必填、全部豁免后条款转为通过、撤销、报告格式菜单、规范库例外列表、删除例外并保存后内置规范回到未改状态。
- 四种报告按前端地址下载后解压核对：工作表、列序、豁免和人工确认内容齐全。

## 待验证（真实运行）

- 用 Word、Excel 实际打开导出的文件（目前只解压核对了内容）。
- 在界面上点击下载并保存文件，含桌面端。
- `dblens-desktop` 这次没有编译：本机 `webview2-com-sys` 的构建脚本找不到文件，与本次改动无关。
- 五种数据库实库检查仍按 M7a 的待验证项。
