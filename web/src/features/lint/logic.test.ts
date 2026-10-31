import { describe, expect, it } from 'vitest'

import type { LintClauseResult, LintRun } from '../../shared/api'
import { diffRuns, effectiveVerdict, groupBySection, inOrder, reviewable, summarize, verdictTone } from './logic'

function result(clause: string, verdict: LintClauseResult['verdict'], issues = 0): LintClauseResult {
  return {
    clause,
    verdict,
    issues: Array.from({ length: issues }, (_, i) => ({ object: `o${i}`, message: 'm' })),
  }
}

describe('effectiveVerdict', () => {
  it('prefers the manual review when present', () => {
    const r = { ...result('c', 'unknown'), review: { verdict: 'pass' as const, note: '' } }
    expect(effectiveVerdict(r)).toBe('pass')
    expect(effectiveVerdict(result('c', 'fail'))).toBe('fail')
  })
})

describe('diffRuns', () => {
  const run = (results: LintClauseResult[]): LintRun =>
    ({
      id: 'r',
      created_at: 0,
      target: 't',
      ruleset_id: 's',
      ruleset_name: 's',
      family: 'mysql',
      clauses: results.map((r) => ({ id: r.clause, section: 'table', text: `text ${r.clause}`, severity: 'error' })),
      results,
    }) as LintRun

  it('matches issues by clause and object, ignoring the message', () => {
    const a = result('a', 'fail', 2)
    const b = { ...result('a', 'fail', 3) }
    b.issues[0] = { ...b.issues[0], message: 'translated' }
    const d = diffRuns(run([a]), run([b]))
    expect(d.verdicts).toEqual([])
    expect(d.added.map((x) => x.issue.object)).toEqual(['o2'])
    expect(d.resolved).toEqual([])
  })

  it('reports verdict changes, resolved issues and clauses present on one side', () => {
    const d = diffRuns(run([result('a', 'fail', 1), result('gone', 'pass')]), run([result('a', 'pass'), result('new', 'manual')]))
    expect(d.verdicts).toEqual([
      { clause: 'a', text: 'text a', before: 'fail', after: 'pass' },
      { clause: 'new', text: 'text new', before: undefined, after: 'manual' },
      { clause: 'gone', text: 'text gone', before: 'pass', after: undefined },
    ])
    expect(d.resolved.map((x) => x.issue.object)).toEqual(['o0'])
  })
})

describe('inOrder', () => {
  it('sorts the selected runs oldest first', () => {
    const r = (id: string, created_at: number) => ({ id, created_at, target: '', ruleset_name: '' })
    expect(inOrder([r('b', 2), r('a', 1), r('c', 3)], ['b', 'a']).map((x) => x.id)).toEqual(['a', 'b'])
  })
})

describe('summarize waivers', () => {
  it('counts waived issues apart from open issues', () => {
    const r = { ...result('c', 'pass'), waived: [{ object: 'o', message: 'm', reason: 'r' }] }
    const s = summarize([r, result('d', 'fail', 2)])
    expect([s.issues, s.waived]).toEqual([2, 1])
  })
})

describe('reviewable', () => {
  it('is true only for unknown and manual', () => {
    expect(reviewable('unknown')).toBe(true)
    expect(reviewable('manual')).toBe(true)
    expect(reviewable('pass')).toBe(false)
    expect(reviewable('fail')).toBe(false)
    expect(reviewable('not_applicable')).toBe(false)
  })
})

describe('summarize', () => {
  it('counts verdicts and rate over pass+fail only', () => {
    const s = summarize([
      result('a', 'pass'),
      result('b', 'pass'),
      result('c', 'fail', 2),
      result('d', 'unknown'),
      result('e', 'manual'),
      result('f', 'not_applicable'),
    ])
    expect([s.pass, s.fail, s.unknown, s.manual, s.not_applicable]).toEqual([2, 1, 1, 1, 1])
    expect(s.issues).toBe(2)
    expect(s.passRate).toBe(66.7)
  })

  it('uses the manual verdict and leaves rate null without decided clauses', () => {
    const reviewed = { ...result('a', 'unknown'), review: { verdict: 'not_applicable' as const, note: '' } }
    const s = summarize([reviewed, result('b', 'manual')])
    expect(s.not_applicable).toBe(1)
    expect(s.manual).toBe(1)
    expect(s.passRate).toBe(null)
  })
})

describe('verdictTone', () => {
  it('maps verdicts to tones', () => {
    expect(verdictTone('pass')).toBe('ok')
    expect(verdictTone('fail')).toBe('bad')
    expect(verdictTone('manual')).toBe('warn')
    expect(verdictTone('not_applicable')).toBe('muted')
  })
})

describe('groupBySection', () => {
  it('groups in section order and keeps meta', () => {
    const run: LintRun = {
      id: 'r',
      created_at: 0,
      target: 't',
      ruleset_id: 'general',
      ruleset_name: 'g',
      family: 'postgres',
      clauses: [
        { id: 'c1', section: 'column', text: 'col', severity: 'warning' },
        { id: 'n1', section: 'naming', text: 'name', severity: 'error' },
      ],
      results: [result('c1', 'pass'), result('n1', 'fail', 1)],
    }
    const groups = groupBySection(run)
    expect(groups.map((g) => g.section)).toEqual(['naming', 'column'])
    expect(groups[0].rows[0].meta?.text).toBe('name')
  })
})
