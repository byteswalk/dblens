import { describe, expect, it } from 'vitest'

import type { ErTable } from '../../shared/api'
import { EMPTY_EDITS, OTHER_TOPIC, deleteTopic, moveTables, resolveTopics, setTopicTables, topicOfTable } from './topics'

const t = (name: string, comment: string | null = null) =>
  ({ table: { name, comment, columns: [] }, logical_keys: [] }) as unknown as ErTable

const names = ['SZSS_BASE_A', 'SZSS_BASE_B', 'SZSS_RISK_A', 'SZSS_RISK_B', 'SZSS_LOG_A', 'OTHER']

describe('业务主题', () => {
  it('去掉公共前缀，已知的词用中文，只有一张的和分不出的归其他', () => {
    const list = resolveTopics(names.map((n) => t(n)), EMPTY_EDITS, '其他')
    const of = topicOfTable(list)
    expect(of.get('SZSS_BASE_A')).toBe('基础数据')
    expect(of.get('SZSS_LOG_A')).toBe('其他')
    expect(of.get('OTHER')).toBe('其他')
    expect(list.at(-1)!.id).toBe(OTHER_TOPIC)
  })

  it('没有对应中文时用表注释的共同开头', () => {
    const list = resolveTopics([t('SZSS_RISK_A', '风险研判规则'), t('SZSS_RISK_B', '风险研判结果'), t('SZSS_X_A'), t('SZSS_X_B')], EMPTY_EDITS, '其他')
    expect(topicOfTable(list).get('SZSS_RISK_A')).toBe('风险研判')
  })

  it('用户的改名和移动优先，删除主题后表回到其他', () => {
    const tables = names.map((n) => t(n))
    const renamed = { names: { RISK: '分析研判' }, tables: {} }
    const moved = moveTables(renamed, ['SZSS_BASE_A'], 'RISK')
    expect(topicOfTable(resolveTopics(tables, moved, '其他')).get('SZSS_BASE_A')).toBe('分析研判')
    const risk = resolveTopics(tables, moved, '其他').find((x) => x.id === 'RISK')!
    const removed = deleteTopic(moved, risk)
    expect(removed.tables.SZSS_BASE_A).toBe(OTHER_TOPIC)
  })

  it('删除自动分出的主题后，它不会再冒出来', () => {
    const tables = names.map((n) => t(n))
    const base = resolveTopics(tables, EMPTY_EDITS, '其他').find((x) => x.id === 'BASE')!
    const list = resolveTopics(tables, deleteTopic(EMPTY_EDITS, base), '其他')
    expect(list.some((x) => x.id === 'BASE')).toBe(false)
    expect(topicOfTable(list).get('SZSS_BASE_A')).toBe('其他')
  })

  it('编辑主题包含的表：勾上的移入，去掉的归其他，空主题保留', () => {
    const tables = names.map((n) => t(n))
    const base = resolveTopics(tables, EMPTY_EDITS, '其他').find((x) => x.id === 'BASE')!
    const edited = setTopicTables(EMPTY_EDITS, base, ['SZSS_BASE_A', 'OTHER'])
    const of = topicOfTable(resolveTopics(tables, edited, '其他'))
    expect(of.get('OTHER')).toBe('基础数据')
    expect(of.get('SZSS_BASE_B')).toBe('其他')
    const empty = resolveTopics(tables, setTopicTables(EMPTY_EDITS, base, []), '其他')
    expect(empty.find((x) => x.id === 'BASE')?.name).toBe('基础数据')
  })
})
