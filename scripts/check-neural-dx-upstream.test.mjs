import assert from 'node:assert/strict'
import test from 'node:test'
import { parseRoutes, routeDiff } from './check-neural-dx-upstream.mjs'

test('extracts Flask route, get and post decorators once', () => {
  const source = `
    @app.route('/one')
    @app.get("/two")
    @app.post('/three', methods=['POST'])
    @app.route('/one', methods=['POST'])
  `
  assert.deepEqual(parseRoutes(source), ['/one', '/three', '/two'])
})

test('reports additions and removals independently', () => {
  assert.deepEqual(routeDiff(['/one', '/two'], ['/two', '/three']), {
    added: ['/three'],
    removed: ['/one'],
  })
})

test('accepts an exact positive-control inventory', () => {
  assert.deepEqual(routeDiff(['/one', '/two'], ['/one', '/two']), { added: [], removed: [] })
})

test('fails closed on the negative-control addition', () => {
  assert.deepEqual(routeDiff(['/one'], ['/one', '/future-capability']), {
    added: ['/future-capability'],
    removed: [],
  })
})
