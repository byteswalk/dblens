// 选文件（M2b 设计文档 3.2 节）：桌面版用系统的打开对话框，直接用本机路径；浏览器模式选文件后上传，显示上传进度。

import { App, Button, Progress } from 'antd'
import { IconFile, IconUpload } from '@tabler/icons-react'
import { useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'

import { useApi } from '../../app/context'
import type { FileSource } from '../../shared/api'
import { desktop } from '../../shared/shell'
import { fileNameOf, sizeText } from './logic'

export interface PickedFile {
  source: FileSource
  name: string
  /** 浏览器模式上传后的大小；桌面版不知道。 */
  size: number | null
}

export default function FileField({
  value,
  onChange,
  filters,
}: {
  value: PickedFile | null
  onChange(f: PickedFile): void
  /** 文件类型：第一组用于浏览器的 accept。 */
  filters: { name: string; extensions: string[] }[]
}) {
  const api = useApi()
  const { t } = useTranslation()
  const { message } = App.useApp()
  const input = useRef<HTMLInputElement>(null)
  const [progress, setProgress] = useState<number | null>(null)

  const pickDesktop = async () => {
    const path = await desktop?.openDialog(filters)
    if (path) onChange({ source: { path }, name: fileNameOf(path), size: null })
  }

  const upload = async (file: File) => {
    setProgress(0)
    try {
      const info = await api.upload(file, (r) => setProgress(Math.round(r * 100)))
      onChange({ source: { upload: info.id }, name: info.name, size: info.size })
    } catch (e) {
      message.error((e as Error).message)
    } finally {
      setProgress(null)
    }
  }

  const accept = filters[0]?.extensions.filter((e) => e !== '*').map((e) => `.${e}`).join(',')
  return (
    <div className="row" style={{ gap: 12, flexWrap: 'wrap' }}>
      <Button
        icon={<IconUpload size={16} />}
        loading={progress !== null}
        onClick={() => (desktop ? void pickDesktop() : input.current?.click())}
      >
        {value ? t('import.file.change') : t('import.file.choose')}
      </Button>
      {!desktop && (
        <input
          ref={input}
          type="file"
          accept={accept || undefined}
          style={{ display: 'none' }}
          onChange={(e) => {
            const f = e.target.files?.[0]
            e.target.value = ''
            if (f) void upload(f)
          }}
        />
      )}
      {progress !== null && <Progress percent={progress} size="small" style={{ width: 200 }} />}
      {value && progress === null && (
        <span className="row" style={{ gap: 6 }}>
          <IconFile size={16} />
          {value.name}
          {value.size !== null && <span className="muted">{sizeText(value.size)}</span>}
        </span>
      )}
    </div>
  )
}
