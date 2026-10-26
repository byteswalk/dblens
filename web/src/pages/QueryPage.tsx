import { Alert, App, Button, Input, Select, Space, Table, Typography } from 'antd'
import { useEffect, useRef, useState } from 'react'

import { type Api, type Profile, type SessionInfo, errorText } from '../shared/api'
import { DEFAULT_MAX_ROWS, ExecSocket, type ExecState, cellText, initialExec, reduceExec } from '../shared/exec'

/** 打开会话，经 WebSocket 执行 SQL，验证执行协议（分批、取消、确认）。 */
export default function QueryPage({ api }: { api: Api }) {
  const { message } = App.useApp()
  const [profiles, setProfiles] = useState<Profile[]>([])
  const [profileId, setProfileId] = useState<string>()
  const [session, setSession] = useState<SessionInfo | null>(null)
  const [sql, setSql] = useState('select 1')
  const [state, setState] = useState<ExecState>(initialExec)
  const [running, setRunning] = useState(false)
  const socket = useRef<ExecSocket | null>(null)
  const submitted = useRef('')

  useEffect(() => {
    api.profiles().then(setProfiles, (e: Error) => message.error(e.message))
  }, [api, message])

  // 离开页面时关闭连接和会话
  useEffect(
    () => () => {
      socket.current?.close()
    },
    [],
  )

  const open = async () => {
    if (!profileId) return
    try {
      if (session) await api.closeSession(session.id)
      socket.current?.close()
      const s = await api.openSession(profileId)
      socket.current = new ExecSocket(
        api.wsUrl(s.id),
        (ev) => {
          setState((st) => reduceExec(st, ev, DEFAULT_MAX_ROWS))
          if (ev.type === 'done' || ev.type === 'error') setRunning(false)
        },
        () => setRunning(false),
      )
      setSession(s)
    } catch (e) {
      message.error((e as Error).message)
    }
  }

  const exec = (text: string, confirmed = false) => {
    submitted.current = text
    setState(initialExec)
    setRunning(true)
    socket.current?.exec(text, confirmed).catch((e: Error) => message.error(e.message))
  }

  const confirmed = state.statements.find((s) => s.confirm)
  return (
    <Space direction="vertical" style={{ width: '100%' }}>
      <Space>
        <Select
          style={{ width: 280 }}
          placeholder="选择已保存的连接"
          value={profileId}
          onChange={setProfileId}
          options={profiles.map((p) => ({ value: p.id, label: p.name }))}
        />
        <Button onClick={open} disabled={!profileId}>
          打开会话
        </Button>
        {session && <Typography.Text type="secondary">会话 {session.id}（{session.variant}）</Typography.Text>}
      </Space>
      <Input.TextArea rows={6} value={sql} onChange={(e) => setSql(e.target.value)} style={{ fontFamily: 'monospace' }} />
      <Space>
        <Button type="primary" disabled={!session || running} onClick={() => exec(sql)}>
          执行
        </Button>
        <Button disabled={!state.waitingForMore} onClick={() => socket.current?.fetchMore()}>
          取下一批
        </Button>
        <Button danger disabled={!running} onClick={() => socket.current?.cancel()}>
          取消
        </Button>
      </Space>
      {state.protocolError && <Alert type="error" message={errorText(state.protocolError)} />}
      {confirmed?.confirm && (
        <Alert
          type="warning"
          message={`需要确认：${confirmed.confirm.reason}`}
          action={
            <Button size="small" onClick={() => exec(submitted.current.slice(confirmed.confirm!.start), true)}>
              确认并继续执行
            </Button>
          }
        />
      )}
      {state.statements.map((st) => (
        <div key={st.index}>
          <Typography.Text strong>
            #{st.index + 1} {st.keyword}
          </Typography.Text>
          {st.elapsedMs !== null && <Typography.Text type="secondary">　{st.elapsedMs} ms</Typography.Text>}
          {st.affected !== null && <Typography.Text>　影响 {st.affected} 行</Typography.Text>}
          {st.error && <Alert type="error" message={errorText(st.error)} />}
          {st.messages.map((m, i) => (
            <Alert key={i} type="info" message={m} />
          ))}
          {st.results.map((r, i) => (
            <Table
              key={i}
              size="small"
              bordered
              scroll={{ x: true }}
              pagination={false}
              rowKey={(_, idx) => String(idx)}
              dataSource={r.rows.map((row) => ({ row }))}
              columns={r.columns.map((c, ci) => ({
                title: c.name,
                key: ci,
                render: (_: unknown, rec: { row: (typeof r.rows)[number] }) => cellText(rec.row[ci]),
              }))}
              footer={() => `${r.rows.length} 行${r.complete ? '' : '（还有更多）'}`}
            />
          ))}
        </div>
      ))}
    </Space>
  )
}
