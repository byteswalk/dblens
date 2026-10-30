// 生成 clients-staging/：桥接 jar、jlink 精简 JRE，以及清单里列出的 JDBC 驱动（来自 DBLENS_CLIENTS_DIR）。
// 第三方文件不进仓库。清单里的文件缺失或校验和不符时失败，不产出残缺的包。
import { execFileSync } from 'node:child_process'
import { createHash } from 'node:crypto'
import { copyFileSync, existsSync, mkdirSync, readFileSync, rmSync } from 'node:fs'
import { join } from 'node:path'
import { fileURLToPath } from 'node:url'

const root = fileURLToPath(new URL('..', import.meta.url))
const manifest = JSON.parse(readFileSync(join(root, 'scripts/clients.manifest.json'), 'utf8'))
const stage = join(root, 'clients-staging')
const bridge = join(root, 'bridge/build/dblens-bridge.jar')
if (!existsSync(bridge)) throw new Error('先运行 node scripts/build-bridge.mjs')

rmSync(stage, { recursive: true, force: true })
mkdirSync(join(stage, 'bridge'), { recursive: true })
mkdirSync(join(stage, 'jdbc'), { recursive: true })
copyFileSync(bridge, join(stage, 'bridge/dblens-bridge.jar'))
execFileSync(
  'jlink',
  [
    '--add-modules',
    manifest.modules.join(','),
    '--strip-debug',
    '--no-header-files',
    '--no-man-pages',
    '--compress',
    'zip-6',
    '--output',
    join(stage, 'jre'),
  ],
  { stdio: 'inherit' },
)
for (const j of manifest.jdbc) {
  const dir = process.env.DBLENS_CLIENTS_DIR
  if (!dir) throw new Error('清单里有 JDBC 驱动，需要设置 DBLENS_CLIENTS_DIR')
  const from = join(dir, j.file)
  const sha = createHash('sha256').update(readFileSync(from)).digest('hex')
  if (sha !== j.sha256) throw new Error(`${j.file} 的校验和不符`)
  copyFileSync(from, join(stage, 'jdbc', j.file))
}
