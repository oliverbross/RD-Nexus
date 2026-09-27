#!/usr/bin/env node

import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import { discoverExpressRoutes } from './openhamclock-route-inventory.mjs'

const fixture = `
app.get('/single', handler)
app.get('*', fallback)
app.get(['/.well-known/security.txt', '/security.txt'], security)
`
assert.deepEqual(discoverExpressRoutes(fixture, 'fixture.js').map(({ method, path }) => `${method} ${path}`), [
  'GET /single', 'GET *', 'GET /.well-known/security.txt', 'GET /security.txt',
])

const server = readFileSync(new URL('../third_party/openhamclock/server.js', import.meta.url), 'utf8')
const admin = readFileSync(new URL('../third_party/openhamclock/server/routes/admin.js', import.meta.url), 'utf8')
assert.ok(discoverExpressRoutes(server, 'server.js').some((route) => route.method === 'GET' && route.path === '*'))
const security = discoverExpressRoutes(admin, 'server/routes/admin.js').filter((route) => route.path.includes('security.txt'))
assert.deepEqual(security.map((route) => route.path), ['/.well-known/security.txt', '/security.txt'])
process.stdout.write('OpenHamClock route inventory includes wildcard and array-form Express declarations.\n')
