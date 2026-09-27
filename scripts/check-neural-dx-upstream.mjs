#!/usr/bin/env node
import { execFileSync } from 'node:child_process'
import { readFileSync } from 'node:fs'
import { dirname, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'

const here = dirname(fileURLToPath(import.meta.url))
const manifestPath = resolve(here, '../docs/neural-dx/upstream.json')

export function parseRoutes(source) {
  const routes = new Set()
  const decorator = /^\s*@app\.(?:route|get|post)\(\s*['"]([^'"]+)['"]/gm
  for (const match of source.matchAll(decorator)) routes.add(match[1])
  return [...routes].sort()
}

export function routeDiff(expected, actual) {
  const oldSet = new Set(expected)
  const newSet = new Set(actual)
  return {
    added: actual.filter((route) => !oldSet.has(route)),
    removed: expected.filter((route) => !newSet.has(route)),
  }
}

function argValue(name) {
  const index = process.argv.indexOf(name)
  return index >= 0 ? process.argv[index + 1] : undefined
}

export function main() {
  const manifest = JSON.parse(readFileSync(manifestPath, 'utf8'))
  const offline = process.argv.includes('--offline')
  const json = process.argv.includes('--json')
  const sourceDir = argValue('--source') ?? process.env.NEURAL_DX_UPSTREAM
  let remoteHead = null
  let routeStatus = null
  let fileStatus = null

  if (!offline) {
    const output = execFileSync('git', ['ls-remote', manifest.repository, `refs/heads/${manifest.branch}`], {
      encoding: 'utf8',
      stdio: ['ignore', 'pipe', 'inherit'],
    }).trim()
    remoteHead = output.split(/\s+/)[0] || null
  }

  if (sourceDir) {
    const source = readFileSync(resolve(sourceDir, 'webapp.py'), 'utf8')
    routeStatus = routeDiff([...manifest.routes].sort(), parseRoutes(source))
    const files = execFileSync('git', ['-C', sourceDir, 'ls-files'], { encoding: 'utf8' })
      .trim().split('\n').filter(Boolean).sort()
    fileStatus = routeDiff([...manifest.files].sort(), files)
  }

  const result = {
    repository: manifest.repository,
    pinned: manifest.commit,
    remoteHead,
    upToDate: remoteHead == null ? null : remoteHead === manifest.commit,
    license: manifest.license,
    routes: routeStatus,
    files: fileStatus,
  }

  if (json) console.log(JSON.stringify(result, null, 2))
  else {
    console.log(`Neural DX upstream pin: ${manifest.commit}`)
    if (remoteHead) console.log(remoteHead === manifest.commit ? 'UP_TO_DATE' : `UPDATE_AVAILABLE ${remoteHead}`)
    if (routeStatus) {
      console.log(`ROUTES added=${routeStatus.added.length} removed=${routeStatus.removed.length}`)
      for (const route of routeStatus.added) console.log(`+ ${route}`)
      for (const route of routeStatus.removed) console.log(`- ${route}`)
    }
    if (fileStatus) {
      console.log(`FILES added=${fileStatus.added.length} removed=${fileStatus.removed.length}`)
      for (const path of fileStatus.added) console.log(`+ ${path}`)
      for (const path of fileStatus.removed) console.log(`- ${path}`)
    }
    if (!manifest.license.vendoringAllowed) console.log(`LICENCE_REVIEW_REQUIRED ${manifest.license.reason}`)
  }

  if ((routeStatus && (routeStatus.added.length || routeStatus.removed.length)) ||
      (fileStatus && (fileStatus.added.length || fileStatus.removed.length))) process.exitCode = 2
  else if (remoteHead && remoteHead !== manifest.commit) process.exitCode = 3
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) main()
