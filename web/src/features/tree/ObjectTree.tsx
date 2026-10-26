// 数据库对象树（设计文档 7.3 节；M1 设计稿第 1、3 部分）：所有连接在一棵树里，可以同时展开多个。
// 懒加载：展开时向后端取下一层（后端有元数据缓存，"刷新"会先清后端缓存再重新取）。顶部过滤框按名称或注释过滤已加载的对象。
// 右键菜单（M1a）：连接 —— 编辑、复制、删除、刷新；其他节点 —— 复制名称、刷新。"查看 DDL""生成 SQL"等在 M1c 加入。

import { App, Dropdown, Input, type MenuProps, Tree } from 'antd'
import type { DataNode, EventDataNode } from 'antd/es/tree'
import {
  IconColumns,
  IconEye,
  IconFolder,
  IconFunction,
  IconKey,
  IconLink,
  IconListNumbers,
  IconSchema,
  IconSearch,
  IconTable,
} from '@tabler/icons-react'
import { useQuery, useQueryClient } from '@tanstack/react-query'
import { type Key, type ReactNode, useCallback, useMemo, useState } from 'react'
import { useTranslation } from 'react-i18next'

import { useApi } from '../../app/context'
import { useUi } from '../../app/ui'
import { useQueryActions } from '../editor/actions'
import type { Api, Environment, Profile, RoutineInfo } from '../../shared/api'
import DbIcon from '../../shared/DbIcon'
import { driverLabel } from '../connections/form'
import { variantName } from '../editor/dialect'
import { withVault } from '../connections/vault'
import { metaKey } from '../sql/metadata'
import {
  type CategoryKind,
  type NodeKey,
  categoriesFor,
  categoryLabel,
  decodeKey,
  encodeKey,
  isWithin,
  matches,
  qualifiedName,
  routineKindLabel,
} from './nodes'

/**
 * 已加载的一层子节点。`items` 中的 key 为编码后的节点键。
 * 分类、系统分组的名称和程序类型在显示时按当前界面语言翻译，切换语言后不必重新加载。
 */
interface Loaded {
  items: {
    key: string
    label: string
    comment?: string | null
    extra?: string
    routineKind?: RoutineInfo['kind']
    leaf: boolean
    pk?: boolean
  }[]
  error?: string
}

const ENV_COLOR: Record<Environment, string> = {
  none: 'var(--env-none)',
  dev: 'var(--env-dev)',
  test: 'var(--env-test)',
  prod: 'var(--env-prod)',
}

const CATEGORY_ICON: Record<CategoryKind, ReactNode> = {
  tables: <IconTable size={14} />,
  views: <IconEye size={14} />,
  materialized_views: <IconEye size={14} />,
  routines: <IconFunction size={14} />,
  sequences: <IconListNumbers size={14} />,
  synonyms: <IconLink size={14} />,
}

/** 按节点键取下一层。 */
async function loadChildren(api: Api, profiles: Profile[], k: NodeKey): Promise<Loaded['items']> {
  switch (k.type) {
    case 'c':
    case 'g': {
      const schemas = await api.schemas(k.profileId)
      const pick = schemas.filter((s) => s.is_system === (k.type === 'g'))
      const items: Loaded['items'] = pick.map((s) => ({
        key: encodeKey({ type: 's', profileId: k.profileId, schema: s.name }),
        label: s.name,
        leaf: false,
      }))
      if (k.type === 'c' && schemas.some((s) => s.is_system)) {
        items.push({ key: encodeKey({ type: 'g', profileId: k.profileId }), label: '', leaf: false })
      }
      return items
    }
    case 's': {
      const driver = profiles.find((p) => p.id === k.profileId)?.driver ?? 'mysql'
      return categoriesFor(driver).map((c) => ({
        key: encodeKey({ type: 'k', profileId: k.profileId, schema: k.schema, kind: c.kind }),
        label: c.label,
        leaf: false,
      }))
    }
    case 'k': {
      const obj = (name: string, comment: string | null, extra?: string, routineKind?: RoutineInfo['kind']) => ({
        key: encodeKey({ type: 'o', profileId: k.profileId, schema: k.schema, kind: k.kind, name }),
        label: name,
        comment,
        extra,
        routineKind,
        leaf: k.kind !== 'tables',
      })
      if (k.kind === 'tables' || k.kind === 'views' || k.kind === 'materialized_views') {
        return (await api.objects(k.profileId, k.schema, k.kind)).map((o) => obj(o.name, o.comment))
      }
      if (k.kind === 'routines') {
        return (await api.routines(k.profileId, k.schema)).map((r) => obj(r.name, null, undefined, r.kind))
      }
      if (k.kind === 'sequences') return (await api.sequences(k.profileId, k.schema)).map((s) => obj(s.name, null))
      return (await api.synonyms(k.profileId, k.schema)).map((s) =>
        obj(s.name, null, `→ ${s.target_schema ? `${s.target_schema}.` : ''}${s.target_name}${s.db_link ? `@${s.db_link}` : ''}`),
      )
    }
    case 'o': {
      const t = await api.table(k.profileId, k.schema, k.name)
      const pk = new Set(t?.primary_key?.columns ?? [])
      return (t?.columns ?? []).map((c) => ({
        key: encodeKey({ type: 'l', profileId: k.profileId, schema: k.schema, table: k.name, column: c.name }),
        label: c.name,
        comment: c.comment,
        extra: `${c.data_type.native}${c.nullable ? '' : ' not null'}`,
        leaf: true,
        pk: pk.has(c.name),
      }))
    }
    case 'l':
      return []
  }
}

export default function ObjectTree() {
  const api = useApi()
  const qc = useQueryClient()
  const { t } = useTranslation()
  const { message, modal } = App.useApp()
  const { openConnection, selectProfile } = useUi()
  const { newQuery } = useQueryActions()
  const { data: profiles = [], error: profilesError } = useQuery({ queryKey: ['profiles'], queryFn: () => api.profiles() })
  const [loaded, setLoaded] = useState<Record<string, Loaded>>({})
  const [expanded, setExpanded] = useState<Key[]>([])
  const [filter, setFilter] = useState('')
  const [menuNode, setMenuNode] = useState<NodeKey | null>(null)

  const load = useCallback(
    async (key: string) => {
      const k = decodeKey(key)
      try {
        const items = await withVault(api, () => loadChildren(api, profiles, k))
        setLoaded((m) => ({ ...m, [key]: { items } }))
        // 第一次连上时后端记下了识别出的数据库变体：重新取连接列表，换成具体数据库的图标
        if (k.type === 'c' && !profiles.find((p) => p.id === k.profileId)?.variant) {
          void qc.invalidateQueries({ queryKey: ['profiles'] })
        }
      } catch (e) {
        setLoaded((m) => ({ ...m, [key]: { items: [], error: (e as Error).message } }))
      }
    },
    [api, profiles, qc],
  )

  /** 刷新一个节点：清后端元数据缓存，丢掉它下面已加载和已展开的内容，再重新取。系统分组按整个连接刷新。 */
  const refresh = async (node: NodeKey) => {
    const k: NodeKey = node.type === 'g' ? { type: 'c', profileId: node.profileId } : node
    const key = encodeKey(k)
    await api.refresh(k.profileId, 'schema' in k ? k.schema : null).catch(() => {})
    // 补全用的元数据一并失效
    void qc.invalidateQueries({ queryKey: metaKey(k.profileId, 'schema' in k ? k.schema : undefined) })
    const under = (x: string) => isWithin(decodeKey(x), k)
    setLoaded((m) => Object.fromEntries(Object.entries(m).filter(([x]) => x !== key && !under(x))))
    setExpanded((e) => e.filter((x) => !under(String(x))))
    if (k.type === 'c') await qc.invalidateQueries({ queryKey: ['profiles'] })
    await load(key)
  }

  const treeData = useMemo<DataNode[]>(() => {
    const build = (parent: string, depth: number): DataNode[] | undefined => {
      const l = loaded[parent]
      if (!l) return undefined
      if (l.error) {
        return [{ key: `${parent}\u0002err`, isLeaf: true, selectable: false, title: <span className="err">{l.error}</span> }]
      }
      const items = l.items.filter((i) => {
        const type = decodeKey(i.key).type
        return (type !== 'o' && type !== 'l') || matches(filter, i.label, i.comment)
      })
      return items.map((i) => {
        const k = decodeKey(i.key)
        const icon =
          k.type === 's' ? <IconSchema size={14} /> : k.type === 'g' || k.type === 'k' ? <IconFolder size={14} /> : k.type === 'o' ? CATEGORY_ICON[k.kind] : i.pk ? <IconKey size={14} color="var(--warn)" /> : <IconColumns size={14} />
        const label = k.type === 'k' ? categoryLabel(k.kind) : k.type === 'g' ? t('tree.node.system') : i.label
        const extra = i.routineKind ? routineKindLabel(i.routineKind) : i.extra
        const count = k.type === 'k' && loaded[i.key] && !loaded[i.key].error ? loaded[i.key].items.length : null
        return {
          key: i.key,
          isLeaf: i.leaf,
          icon,
          title: (
            <span>
              {label}
              {count !== null && <span className="cm">{count}</span>}
              {extra && <span className="cm">{extra}</span>}
              {i.comment && <span className="cm">{i.comment}</span>}
            </span>
          ),
          children: build(i.key, depth + 1),
        }
      })
    }
    return profiles.map((p) => {
      const key = encodeKey({ type: 'c', profileId: p.id })
      const env = p.environment ?? 'none'
      return {
        key,
        isLeaf: false,
        title: (
          <span>
            <span className="dot" style={{ background: p.color || ENV_COLOR[env] }} />
            <DbIcon driver={p.driver} variant={p.variant} />{' '}
            <b style={env === 'prod' ? { color: 'var(--danger)' } : undefined}>{p.name}</b>
            {env !== 'none' && <span className={`env ${env}`}>{t(`tree.env.${env}`)}</span>}
            <span className="cm">{variantName(p.variant) ?? driverLabel(p.driver)}</span>
          </span>
        ),
        children: build(key, 1),
      }
    })
  }, [profiles, loaded, filter, t])

  const menuItems: MenuProps['items'] =
    menuNode?.type === 'c'
      ? [
          { key: 'new-query', label: t('tree.menu.newQuery'), extra: 'Ctrl+T' },
          { type: 'divider' },
          { key: 'edit', label: t('tree.menu.editConnection') },
          { key: 'duplicate', label: t('tree.menu.duplicateConnection') },
          { key: 'delete', label: t('tree.menu.deleteConnection'), danger: true },
          { type: 'divider' },
          { key: 'refresh', label: t('tree.menu.refresh') },
        ]
      : menuNode
        ? [
            ...(qualifiedName(menuNode) ? [{ key: 'copy', label: t('tree.menu.copyName') }] : []),
            { key: 'refresh', label: t('tree.menu.refresh') },
          ]
        : []

  const onMenu = async (key: string) => {
    const k = menuNode
    if (!k) return
    try {
      if (key === 'new-query') newQuery(k.profileId)
      else if (key === 'edit' && k.type === 'c') openConnection(k.profileId)
      else if (key === 'duplicate' && k.type === 'c') {
        const p = await api.profile(k.profileId)
        const { id: _, ...input } = p
        await api.createProfile({ ...input, name: t('tree.duplicate.name', { name: p.name }) }, null)
        await qc.invalidateQueries({ queryKey: ['profiles'] })
        message.success(t('tree.duplicate.done'))
      } else if (key === 'delete' && k.type === 'c') {
        const p = profiles.find((x) => x.id === k.profileId)
        modal.confirm({
          title: t('tree.delete.title', { name: p?.name }),
          content: t('tree.delete.content'),
          okType: 'danger',
          okText: t('tree.delete.ok'),
          onOk: async () => {
            await api.deleteProfile(k.profileId)
            await qc.invalidateQueries({ queryKey: ['profiles'] })
          },
        })
      } else if (key === 'copy') {
        await navigator.clipboard.writeText(qualifiedName(k))
        message.success(t('tree.copied'))
      } else if (key === 'refresh') {
        // 列节点刷新其所在的表
        const target: NodeKey =
          k.type === 'l' ? { type: 'o', profileId: k.profileId, schema: k.schema, kind: 'tables', name: k.table } : k
        await refresh(target)
      }
    } catch (e) {
      message.error((e as Error).message)
    }
  }

  return (
    <div className="tree" style={{ display: 'flex', flexDirection: 'column', height: '100%' }}>
      <div style={{ padding: 6 }}>
        <Input
          size="small"
          allowClear
          prefix={<IconSearch size={14} className="muted" />}
          placeholder={t('tree.filterPlaceholder')}
          value={filter}
          onChange={(e) => setFilter(e.target.value)}
        />
      </div>
      {profilesError && <div className="err" style={{ padding: 8 }}>{(profilesError as Error).message}</div>}
      <Dropdown trigger={['contextMenu']} menu={{ items: menuItems, onClick: (e) => onMenu(e.key) }}>
        <div className="grow" style={{ overflow: 'auto' }}>
          <Tree
            blockNode
            showIcon
            treeData={treeData}
            expandedKeys={expanded}
            onExpand={(keys) => setExpanded(keys)}
            loadedKeys={Object.keys(loaded)}
            loadData={(node: EventDataNode<DataNode>) => load(String(node.key))}
            onSelect={(_, { node }) => selectProfile(decodeKey(String(node.key)).profileId)}
            onRightClick={({ node }) => {
              const key = String(node.key)
              setMenuNode(key.includes('\u0002') ? null : decodeKey(key))
            }}
          />
        </div>
      </Dropdown>
    </div>
  )
}
