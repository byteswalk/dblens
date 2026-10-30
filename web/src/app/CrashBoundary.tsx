// 渲染出错时显示错误页（否则整个窗口变成空白），并把错误写进日志。

import { Component, type ErrorInfo, type ReactNode } from 'react'

import { reportClientError } from '../shared/crash'
import { t } from '../shared/i18n'

interface State {
  error: Error | null
  details: string
}

export default class CrashBoundary extends Component<{ children: ReactNode }, State> {
  state: State = { error: null, details: '' }

  static getDerivedStateFromError(error: Error): Partial<State> {
    return { error }
  }

  componentDidCatch(error: Error, info: ErrorInfo) {
    const componentStack = info.componentStack ?? ''
    this.setState({ details: `${error.stack ?? error.message}\n${componentStack}` })
    reportClientError({ kind: 'render', message: error.message, stack: error.stack, component_stack: componentStack })
  }

  render() {
    const { error, details } = this.state
    if (!error) return this.props.children
    return (
      <div className="crash">
        <h2>{t('app.crash.title')}</h2>
        <p>{t('app.crash.hint')}</p>
        <pre>{details || error.message}</pre>
        <div className="row crash-actions">
          <button onClick={() => void navigator.clipboard?.writeText(details || error.message)}>
            {t('app.crash.copy')}
          </button>
          <button onClick={() => location.reload()}>{t('app.crash.reload')}</button>
        </div>
      </div>
    )
  }
}
