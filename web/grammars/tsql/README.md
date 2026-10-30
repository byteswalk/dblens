# T-SQL 语法（SQL Server）

来源：[antlr/grammars-v4](https://github.com/antlr/grammars-v4) 的 `sql/tsql`，提交 `afbe4ed460d091afd7158b5cbd4ef63e64523e21`（2026-10-17，与 PL/SQL 语法相同；`sql/tsql` 最后一次改动是 2026-07-05）。许可证 MIT（见 `.g4` 文件头）。

| 文件 | 上游路径 |
|---|---|
| `TSqlLexer.g4`、`TSqlParser.g4` | `sql/tsql/` |

这里的文件与上游一致，不直接修改；语法与目标语言无关，没有基类文件。`web/scripts/gen-tsql.mjs` 用 `antlr-ng` 生成 TypeScript，输出到 `src/features/sql/tsql/gen/`（不提交，`pnpm dev` / `build` / `test` / `typecheck` 前自动生成，输入不变时跳过）。

更新上游版本：按依赖原则（只取发布满 7 天的提交），替换这两个文件并更新上面的提交号。
