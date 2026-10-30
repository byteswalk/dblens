import { describe, expect, it } from 'vitest'

import type { ErTable } from '../../shared/api'
import { EMPTY_EDITS, OTHER_TOPIC, deleteTopic, moveTables, resolveTopics, topicOfTable } from './topics'

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
    const removed = deleteTopic(moved, 'RISK')
    expect(removed.tables.SZSS_BASE_A).toBe(OTHER_TOPIC)
  })
})
