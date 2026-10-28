import { describe, expect, it } from 'vitest'

import { type ErTabState, hasErChanges, useErTabs } from './events'

const state = (tables: ErTabState['tables']): ErTabState => ({
  positions: {},
  display: 'columns',
  tables,
  fingerprint: 'f',
  dirty: false,
  loadedKey: 'e:f',
})

describe('ER 图标签的编辑状态', () => {
  it('设计器保存了从主题图新建的表：加进这张图、标为未保存；默认图不变', () => {
    const s = useErTabs.getState()
    s.set('topic', state(['orders']))
    s.set('all', state('all'))
    s.wait('d1', 'topic')
    s.wait('d2', 'all')
    expect(hasErChanges('topic')).toBe(false)
    s.tableCreated('d1', 'shipments')
    s.tableCreated('d2', 'other')
    // 没有在等的设计器：不处理
    s.tableCreated('d3', 'x')
    const tabs = useErTabs.getState().tabs
    expect(tabs.topic).toMatchObject({ tables: ['orders', 'shipments'], dirty: true })
    expect(tabs.all).toMatchObject({ tables: 'all', dirty: false })
    expect(hasErChanges('topic')).toBe(true)
    expect(useErTabs.getState().waiting).toEqual({})
    s.clear('topic')
    expect(hasErChanges('topic')).toBe(false)
  })
})
