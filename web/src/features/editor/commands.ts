// 编辑器命令：菜单、全局快捷键和工具栏都发命令，由当前标签页的视图执行（它持有编辑器实例）。

export type EditorCommand = 'run-current' | 'run-all' | 'cancel' | 'format' | 'explain'

const bus = new EventTarget()

export function sendCommand(cmd: EditorCommand) {
  bus.dispatchEvent(new CustomEvent<EditorCommand>('command', { detail: cmd }))
}

/** 订阅命令，返回取消订阅的函数。 */
export function onCommand(handler: (cmd: EditorCommand) => void): () => void {
  const listener = (e: Event) => handler((e as CustomEvent<EditorCommand>).detail)
  bus.addEventListener('command', listener)
  return () => bus.removeEventListener('command', listener)
}
