# PL/SQL 语法（Oracle、达梦）

来源：[antlr/grammars-v4](https://github.com/antlr/grammars-v4) 的 `sql/plsql`，提交 `afbe4ed460d091afd7158b5cbd4ef63e64523e21`（2026-10-17）。许可证 Apache-2.0（见 `.g4` 文件头）。

| 文件 | 上游路径 |
|---|---|
| `PlSqlLexer.g4`、`PlSqlParser.g4` | `sql/plsql/` |
| `PlSqlLexerBase.ts`、`PlSqlParserBase.ts` | `sql/plsql/Antlr4ng/` |

这里的文件与上游一致，不直接修改。`web/scripts/gen-plsql.mjs` 生成解析器时：
1. 按上游 `Antlr4ng/transformGrammar.py` 的做法，在语法里插入基类的 import；
2. 打上该脚本 `PATCHES` 中的补丁（达梦语法和上游缺陷，每条都有说明和测试）；
3. 用 `antlr-ng` 生成 TypeScript，输出到 `src/features/sql/plsql/gen/`（不提交，`pnpm dev` / `build` / `test` / `typecheck` 前自动生成，输入不变时跳过）。

更新上游版本：按依赖原则（只取发布满 7 天的提交），替换这四个文件并更新上面的提交号；补丁对不上时脚本会报错，按新语法调整补丁。
