import { readdirSync, readFileSync, statSync } from 'node:fs'
import { join, relative, resolve } from 'node:path'
import { describe, expect, it } from 'vitest'

function productionFiles(root: string): string[] {
  return readdirSync(root).flatMap((name) => {
    const path = join(root, name)
    if (name.includes('.test.') || name === 'confirm.tsx') return []
    return statSync(path).isDirectory() ? productionFiles(path) : [path]
  })
}

describe('macOS confirmation compatibility', () => {
  it('keeps production UI off inert window.confirm', () => {
    const root = resolve(process.cwd(), 'src')
    const offenders = productionFiles(root)
      .filter((path) => /\.(?:ts|tsx)$/.test(path))
      .filter((path) => {
        const source = readFileSync(path, 'utf8')
          .replace(/\/\*[\s\S]*?\*\//g, '')
          .replace(/^\s*\/\/.*$/gm, '')
        return /window\.confirm\s*\(/.test(source)
      })
      .map((path) => relative(root, path))

    expect(offenders).toEqual([])
  })
})
