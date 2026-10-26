// 依赖复查（依赖原则见 docs/superpowers/specs/2026-10-25-m1b2-completion-design.md 3.1 节）：
// 列出前端（web/package.json）和后端（Cargo.toml 的 workspace.dependencies）直接依赖中
// 可以升级的项。只看正式版（版本号不带 `-`、未撤回），发布满 N 天（默认 7）才算可升级。
//
// 用法：node scripts/check-deps.mjs [--days 7]
// 环境变量 NPM_REGISTRY 指定 npm 仓库（默认 https://registry.npmjs.org，国内可用 https://registry.npmmirror.com）。

import { readFileSync } from 'node:fs'
import { dirname, join } from 'node:path'
import { fileURLToPath } from 'node:url'

const root = join(dirname(fileURLToPath(import.meta.url)), '..')
const daysArg = process.argv.indexOf('--days')
const days = daysArg > 0 ? Number(process.argv[daysArg + 1]) : 7
const cutoff = Date.now() - days * 86_400_000
const npmRegistry = (process.env.NPM_REGISTRY ?? 'https://registry.npmjs.org').replace(/\/$/, '')

const isStable = (v) => /^\d+\.\d+\.\d+$/.test(v)
const cmp = (a, b) => {
  const x = a.split('.').map(Number)
  const y = b.split('.').map(Number)
  for (let i = 0; i < 3; i++) if (x[i] !== y[i]) return x[i] - y[i]
  return 0
}

/** 从 [版本, 发布时间] 中选出最新的正式版和其中发布满期限的最新版。 */
function pick(versions) {
  const stable = versions.filter(([v]) => isStable(v)).sort((a, b) => cmp(b[0], a[0]))
  return { newest: stable[0], eligible: stable.find(([, time]) => Date.parse(time) <= cutoff) }
}

async function getJson(url) {
  const resp = await fetch(url, { headers: { 'User-Agent': 'dblens-check-deps (github.com/byteswalk/dblens)' } })
  if (!resp.ok) throw new Error(`${url}: HTTP ${resp.status}`)
  return resp.json()
}

async function npm(name) {
  const d = await getJson(`${npmRegistry}/${name.replace('/', '%2F')}`)
  return pick(Object.keys(d.versions).filter((v) => !d.versions[v].deprecated).map((v) => [v, d.time[v]]))
}

async function crate(name) {
  const d = await getJson(`https://crates.io/api/v1/crates/${name}/versions`)
  return pick(d.versions.filter((v) => !v.yanked).map((v) => [v.num, v.created_at]))
}

function webDeps() {
  const pkg = JSON.parse(readFileSync(join(root, 'web/package.json'), 'utf8'))
  return Object.entries({ ...pkg.dependencies, ...pkg.devDependencies }).map(([name, v]) => ({
    eco: 'npm',
    name,
    current: v.replace(/^[\^~]/, ''),
  }))
}

function cargoDeps() {
  const toml = readFileSync(join(root, 'Cargo.toml'), 'utf8')
  const section = toml.split('[workspace.dependencies]')[1].split(/^\[/m)[0]
  const deps = []
  for (const line of section.split('\n')) {
    const m = line.match(/^([A-Za-z0-9_-]+)\s*=\s*(?:"([^"]+)"|\{.*?version\s*=\s*"([^"]+)")/)
    if (m) deps.push({ eco: 'cargo', name: m[1], current: m[2] ?? m[3] })
  }
  return deps
}

const deps = [...webDeps(), ...cargoDeps()]
const rows = await Promise.all(
  deps.map(async (d) => {
    try {
      const { newest, eligible } = await (d.eco === 'npm' ? npm(d.name) : crate(d.name))
      return { ...d, newest, eligible }
    } catch (e) {
      return { ...d, error: e.message }
    }
  }),
)

let pending = 0
for (const r of rows) {
  if (r.error) {
    console.log(`${r.eco}\t${r.name}\t${r.current}\t查询失败：${r.error}`)
    continue
  }
  if (r.eligible && cmp(r.eligible[0], r.current) > 0) {
    pending++
    console.log(`${r.eco}\t${r.name}\t${r.current} -> ${r.eligible[0]}\t（${r.eligible[1].slice(0, 10)} 发布）`)
  } else if (r.newest && cmp(r.newest[0], r.current) > 0) {
    console.log(`${r.eco}\t${r.name}\t${r.current}\t${r.newest[0]} 未满 ${days} 天（${r.newest[1].slice(0, 10)} 发布），暂不升级`)
  }
}
console.log(`共 ${rows.length} 个直接依赖，${pending} 个可升级`)
