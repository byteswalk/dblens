import { describe, expect, it } from 'vitest'

import type { LintClauseResult, LintRun } from '../../shared/api'
import { effectiveVerdict, groupBySection, reviewable, summarize, verdictTone } from './logic'

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
