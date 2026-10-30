// 编译桥接进程，产物 bridge/build/dblens-bridge.jar。需要 JDK 11 及以上；cargo 构建不依赖它。
import { execFileSync } from 'node:child_process'
import { mkdirSync, readdirSync, rmSync } from 'node:fs'
import { join } from 'node:path'
import { fileURLToPath } from 'node:url'

const root = fileURLToPath(new URL('..', import.meta.url))
const list = (d) => readdirSync(d).filter((f) => f.endsWith('.java')).map((f) => join(d, f))
const src = list(join(root, 'bridge/src/dblens/bridge'))
const test = list(join(root, 'bridge/test/dblens/bridge'))
const out = join(root, 'bridge/build')
rmSync(out, { recursive: true, force: true })
mkdirSync(join(out, 'classes'), { recursive: true })
mkdirSync(join(out, 'test'), { recursive: true })
const run = (cmd, args) => execFileSync(cmd, args, { stdio: 'inherit' })
run('javac', ['--release', '11', '-encoding', 'UTF-8', '-d', join(out, 'classes'), ...src])
run('javac', ['--release', '11', '-encoding', 'UTF-8', '-cp', join(out, 'classes'), '-d', join(out, 'test'), ...test])
run('java', ['-Dfile.encoding=UTF-8', '-cp', `${join(out, 'classes')}${process.platform === 'win32' ? ';' : ':'}${join(out, 'test')}`, 'dblens.bridge.FrameTest'])
run('jar', ['--create', '--file', join(out, 'dblens-bridge.jar'), '--main-class', 'dblens.bridge.Main', '-C', join(out, 'classes'), '.'])
