// 规范检查结果的纯逻辑：结论归并、分组、统计。不依赖 React，便于测试。

import type { LintClauseMeta, LintClauseResult, LintIssue, LintRun, LintRunSummary, LintSection, LintVerdict } from '../../shared/api'

/** 条款分类的显示顺序。 */
export const SECTION_ORDER: LintSection[] = [
  'naming',
  'table',
  'primary_key',
  'column',
  'index',
  'constraint',
  'consistency',
  'distribution',
  'design',
]

/** 条款的最终结论：填过人工结论的以人工为准。 */
export function effectiveVerdict(r: LintClauseResult): LintVerdict {
  return r.review ? r.review.verdict : r.verdict
}

/** 能否填人工结论：无法判断、待人工确认两种。 */
export function reviewable(v: LintVerdict): boolean {
  return v === 'unknown' || v === 'manual'
}

/** 结论对应的色调，界面按它上色。 */
export function verdictTone(v: LintVerdict): 'ok' | 'bad' | 'warn' | 'muted' {
  switch (v) {
    case 'pass':
      return 'ok'
    case 'fail':
      return 'bad'
    case 'unknown':
    case 'manual':
      return 'warn'
    case 'not_applicable':
      return 'muted'
  }
}

export interface LintSummary {
  pass: number
  fail: number
  unknown: number
  manual: number
  not_applicable: number
  issues: number
  /** 已豁免的问题数，不计入 issues。 */
  waived: number
  /** 通过率百分比，保留一位小数；没有可计算的条款时为 null。 */
  passRate: number | null
}

/** 统计各结论数量和通过率。通过率只算通过和未通过。 */
export function summarize(results: LintClauseResult[]): LintSummary {
  const s: LintSummary = {
    pass: 0,
    fail: 0,
    unknown: 0,
    manual: 0,
    not_applicable: 0,
    issues: 0,
    waived: 0,
    passRate: null,
  }
  for (const r of results) {
    s[effectiveVerdict(r)] += 1
    s.issues += r.issues.length
    s.waived += r.waived?.length ?? 0
  }
  const total = s.pass + s.fail
  if (total > 0) s.passRate = Math.round((s.pass * 1000) / total) / 10
  return s
}

/** 记录的时间，按本地时区。 */
export function formatTime(ms: number): string {
  const d = new Date(ms)
  const p = (n: number) => String(n).padStart(2, '0')
  return `${d.getFullYear()}-${p(d.getMonth() + 1)}-${p(d.getDate())} ${p(d.getHours())}:${p(d.getMinutes())}`
}

/** 选中的记录按时间从早到晚排。 */
export function inOrder(runs: LintRunSummary[], ids: string[]): LintRunSummary[] {
  return runs.filter((r) => ids.includes(r.id)).sort((a, b) => a.created_at - b.created_at)
}

export interface DiffVerdict {
  clause: string
  text: string
  before?: LintVerdict
  after?: LintVerdict
}

export interface DiffIssue {
  clause: string
  text: string
  issue: LintIssue
}

export interface RunDiff {
  verdicts: DiffVerdict[]
  added: DiffIssue[]
  resolved: DiffIssue[]
}

/** 对比两次检查。按条款编号和对象名匹配：问题文字是检查时的语言，不参与比较。 */
export function diffRuns(before: LintRun, after: LintRun): RunDiff {
  const text = (clause: string) =>
    after.clauses.find((c) => c.id === clause)?.text ?? before.clauses.find((c) => c.id === clause)?.text ?? clause
  const objects = (r?: LintClauseResult) => new Set(r?.issues.map((i) => i.object))
  const old = new Map(before.results.map((r) => [r.clause, r]))
  const cur = new Map(after.results.map((r) => [r.clause, r]))
  const clauses = [...new Set([...cur.keys(), ...old.keys()])]
  const diff: RunDiff = { verdicts: [], added: [], resolved: [] }
  for (const clause of clauses) {
    const a = old.get(clause)
    const b = cur.get(clause)
    const va = a && effectiveVerdict(a)
    const vb = b && effectiveVerdict(b)
    if (va !== vb) diff.verdicts.push({ clause, text: text(clause), before: va, after: vb })
    const was = objects(a)
    const now = objects(b)
    for (const issue of b?.issues ?? []) if (!was.has(issue.object)) diff.added.push({ clause, text: text(clause), issue })
    for (const issue of a?.issues ?? []) if (!now.has(issue.object)) diff.resolved.push({ clause, text: text(clause), issue })
  }
  return diff
}

/** 浏览器下载；文件名由响应头决定。 */
export function download(url: string) {
  const a = document.createElement('a')
  a.href = url
  a.download = ''
  a.click()
}

export interface SectionGroup {
  section: LintSection
  rows: { meta: LintClauseMeta | undefined; result: LintClauseResult }[]
}

/** 按分类分组，分类内保持原顺序。 */
export function groupBySection(run: LintRun): SectionGroup[] {
  const metaOf = new Map(run.clauses.map((c) => [c.id, c]))
  const groups = new Map<LintSection, SectionGroup['rows']>()
  for (const result of run.results) {
    const meta = metaOf.get(result.clause)
    const section = meta?.section ?? 'design'
    if (!groups.has(section)) groups.set(section, [])
    groups.get(section)!.push({ meta, result })
  }
  return SECTION_ORDER.filter((s) => groups.has(s)).map((section) => ({ section, rows: groups.get(section)! }))
}
