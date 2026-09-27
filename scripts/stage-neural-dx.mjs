#!/usr/bin/env node
import { execFileSync } from 'node:child_process'
import { cpSync, existsSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { basename, dirname, join, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'

const repo = resolve(dirname(fileURLToPath(import.meta.url)), '..')
const source = join(repo, 'third_party', 'neural-dx-watcher')
const integration = join(repo, 'integrations', 'neural-dx')
const manifest = JSON.parse(readFileSync(join(repo, 'docs', 'neural-dx', 'upstream.json'), 'utf8'))
const stage = join(repo, 'src-tauri', 'resources', 'neural-dx')
const builder = join(repo, 'target', `neural-dx-builder-${process.platform}-${process.arch}`)
const python = process.platform === 'win32' ? join(builder, 'Scripts', 'python.exe') : join(builder, 'bin', 'python')

function findBuildPython() {
  const candidates = process.env.PYTHON
    ? [process.env.PYTHON]
    : process.platform === 'win32'
      ? ['python']
      : ['python3.13', 'python3.12', 'python3.11', 'python3.10', 'python3']
  for (const candidate of candidates) {
    try {
      const compatible = execFileSync(candidate, ['-c', 'import sys; print(int(sys.version_info >= (3, 10)))'], { encoding: 'utf8' }).trim()
      if (compatible === '1') return candidate
    } catch {
      // Try the next installed interpreter.
    }
  }
  throw new Error('Neural DX packaging requires Python 3.10 or newer (set PYTHON to a compatible interpreter)')
}

function run(command, args, cwd = repo) {
  execFileSync(command, args, { cwd, stdio: 'inherit' })
}

function replaceOnce(path, before, after) {
  const input = readFileSync(path, 'utf8')
  const first = input.indexOf(before)
  if (first < 0 || input.indexOf(before, first + before.length) >= 0) {
    throw new Error(`Neural DX integration anchor changed: ${path}\nExpected exactly one:\n${before}`)
  }
  writeFileSync(path, input.replace(before, after))
}

if (!existsSync(join(source, 'webapp.py'))) {
  throw new Error('Neural DX submodule is absent; run git submodule update --init third_party/neural-dx-watcher')
}
const actual = execFileSync('git', ['-C', source, 'rev-parse', 'HEAD'], { encoding: 'utf8' }).trim()
if (actual !== manifest.commit) throw new Error(`Neural DX pin mismatch: expected ${manifest.commit}, found ${actual}`)

const temp = mkdtempSync(join(tmpdir(), 'rd-nexus-neural-dx-'))
const work = join(temp, 'source')
try {
  cpSync(source, work, { recursive: true, filter: (path) => basename(path) !== '.git' && basename(path) !== '__pycache__' })
  cpSync(join(integration, 'nexus_bridge.py'), join(work, 'nexus_bridge.py'))
  const webapp = join(work, 'webapp.py')
  replaceOnce(webapp, 'MY_CALL = "F1SMV"\nWEB_PORT = 8000', 'MY_CALL = os.getenv("NEURAL_DX_CALLSIGN", "F1SMV")\nWEB_PORT = int(os.getenv("NEURAL_DX_PORT", "8000"))')
  replaceOnce(webapp, 'DEFAULT_QRA = "JN23"', 'DEFAULT_QRA = os.getenv("NEURAL_DX_GRID", "JN23")')
  replaceOnce(webapp, 'if __name__ == "__main__":\n', 'import sys\nfrom nexus_bridge import install_nexus_bridge\ninstall_nexus_bridge(app, sys.modules[__name__])\n\nif __name__ == "__main__":\n')
  replaceOnce(webapp, '    threading.Thread(target=telnet_worker, daemon=True).start()', '    if os.getenv("NEURAL_DX_NEXUS_EMBED") != "1":\n        threading.Thread(target=telnet_worker, daemon=True).start()')
  replaceOnce(webapp, '    threading.Thread(target=wsjtx_worker, daemon=True).start()', '    if os.getenv("NEURAL_DX_NEXUS_EMBED") != "1":\n        threading.Thread(target=wsjtx_worker, daemon=True).start()')
  replaceOnce(webapp, "    app.run(host='0.0.0.0', port=WEB_PORT, debug=False, use_reloader=False)", "    app.run(host='127.0.0.1' if os.getenv('NEURAL_DX_NEXUS_EMBED') == '1' else '0.0.0.0', port=WEB_PORT, debug=False, use_reloader=False)")

  if (existsSync(python)) {
    const compatible = execFileSync(python, ['-c', 'import sys; print(int(sys.version_info >= (3, 10)))'], { encoding: 'utf8' }).trim()
    if (compatible !== '1') rmSync(builder, { recursive: true, force: true })
  }
  if (!existsSync(python)) {
    mkdirSync(dirname(builder), { recursive: true })
    run(findBuildPython(), ['-m', 'venv', builder])
  }
  run(python, ['-m', 'pip', 'install', '--disable-pip-version-check', '--upgrade', 'pip'])
  run(python, ['-m', 'pip', 'install', '--disable-pip-version-check', '-r', join(work, 'requirements.txt'), 'pyinstaller==6.16.0'])
  const separator = process.platform === 'win32' ? ';' : ':'
  run(python, [
    '-m', 'PyInstaller', '--noconfirm', '--clean', '--onefile', '--name', 'neural-dx-watcher',
    '--add-data', `${join(work, 'templates')}${separator}templates`,
    '--add-data', `${join(work, 'static')}${separator}static`,
    '--hidden-import', 'sgp4.api', '--hidden-import', 'paho.mqtt.client',
    join(work, 'webapp.py'),
  ], temp)

  mkdirSync(stage, { recursive: true })
  const executable = process.platform === 'win32' ? 'neural-dx-watcher.exe' : 'neural-dx-watcher'
  cpSync(join(temp, 'dist', executable), join(stage, executable))
  cpSync(join(source, 'README_EN.md'), join(stage, 'UPSTREAM-README.txt'))
  cpSync(join(repo, 'docs', 'neural-dx', 'SOURCE_PERMISSION.md'), join(stage, 'SOURCE-PERMISSION.txt'))
  writeFileSync(join(stage, 'provenance.json'), `${JSON.stringify({
    repository: manifest.repository,
    commit: manifest.commit,
    version: manifest.version,
    builtAt: new Date().toISOString(),
    platform: process.platform,
    arch: process.arch,
  }, null, 2)}\n`)
  process.stdout.write(`Staged Neural DX ${manifest.version} (${actual}) for ${process.platform}-${process.arch}\n`)
} finally {
  rmSync(temp, { recursive: true, force: true })
}
