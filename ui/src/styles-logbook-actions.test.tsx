// @vitest-environment jsdom
import { describe, it, expect, vi, beforeAll, afterEach } from 'vitest'
import { render, waitFor, cleanup } from '@testing-library/react'
import { readFileSync } from 'node:fs'
import { resolve } from 'node:path'
import { parseRules, specificity, cmpSpec } from './cssCascade'
// Imported here, not inside a test: loading the Logbook module (~0.6 s) then counts against no
// test's time limit. (`vi.mock` below is hoisted above this import either way.)
import { Logbook } from './components/Logbook'
import type { LogQuestion } from './features/logAnswers'
import { t } from './i18n'

// ─────────────────────────────────────────────────────────────────────────────────────────
// THE LOGBOOK ROW'S ACTION CLUSTER MUST FIT ITS GRID TRACK.
//
// The defect this exists to make unrepeatable (operator report, 2026-09-16 — "buttons look
// cramped on the screen"): the 160px floor was written 2026-07-15 against the cluster of the
// day — six controls of 22px each, 6×22 + 5×4 = 152. Three more joined (📢, QRZ✎, WRL), ↥
// became the word QRZ, and the floor never moved. `.log-cell` is `overflow:hidden` with
// `justify-content:flex-end`, so the excess is sheared off the LEFT: at 1366–1920 the harness
// measures 70px of the cluster gone — 📢, QRZ and part of QRZ✎, invisible and unclickable —
// with every surviving label also printing across its neighbour.
//
// WHAT THIS FILE CAN AND CANNOT DO. jsdom lays nothing out: every getBoundingClientRect here
// is 0×0, so it cannot measure a label. ui/layout-harness/measure-logbook-actions.sh does
// that, in real Chrome. This guard does the two things jsdom IS good for — it MOUNTS the real
// component and counts what it renders, and it COMPUTES cascade winners out of the real sheet
// — and joins them:
//
//   1. a LOWER BOUND on the cluster's width, derived from the rendered controls and the
//      resolved CSS box model, using a per-character advance chosen well BELOW any real face.
//      It cannot over-estimate, so it cannot produce a false red — and on origin/main it
//      still comes to 236px against that sheet's 160px floor, which is why this file fails
//      there.
//   2. the INPUTS to the harness measurement, pinned: control count, label set, font-size,
//      padding, gap, and the track count against the cells the component actually renders.
//      Change any of them and this goes red pointing at the harness, because the number in
//      the sheet has stopped describing what is on screen. That is exactly the step that was
//      skipped when the glyphs became words.
//
// Never replace any of this with a regex for the floor's text — a dead selector passes that.
// ─────────────────────────────────────────────────────────────────────────────────────────

// Comments are stripped BEFORE parsing: `parseRules` is brace-aware, not comment-aware, and
// these rules carry their rationale INSIDE the body, where a comment's own colon would be read
// as a declaration. (Resolved from the cwd, not `import.meta.url`: under the jsdom environment
// that is an http URL. Same idiom as the other jsdom style guards.)
const CSS = readFileSync(resolve(process.cwd(), 'src', 'styles.css'), 'utf8').replace(
  /\/\*[\s\S]*?\*\//g,
  '',
)
const RULES = parseRules(CSS)

/**
 * The winning declaration of `prop` on an element matched by any of `selectors`, by the real
 * cascade: important first, then specificity, then source order. The caller names the exact
 * selectors that apply to the element it means — which is what keeps a whole-sheet resolver
 * honest without a DOM to match against.
 */
function winner(selectors: string[], prop: string): string | null {
  let best: { value: string; important: boolean; spec: readonly number[]; order: number } | null = null
  for (const rule of RULES) {
    if (!selectors.includes(rule.selector)) continue
    for (const d of rule.decls) {
      if (d.prop !== prop) continue
      const important = /!important/.test(d.value)
      const cand = {
        value: d.value.replace(/\s*!important\s*/, '').trim(),
        important,
        spec: specificity(rule.selector),
        order: rule.order,
      }
      const beats =
        !best ||
        (cand.important && !best.important) ||
        (cand.important === best.important &&
          (cmpSpec(cand.spec, best.spec) > 0 ||
            (cmpSpec(cand.spec, best.spec) === 0 && cand.order > best.order)))
      if (beats) best = cand
    }
  }
  return best?.value ?? null
}

/** The track list the row gets under `selectors`. `minmax(336px, 0.8fr)` stays one track. */
function tracks(selectors: string[]): string[] {
  const v = winner(selectors, 'grid-template-columns')
  expect(v, `no grid-template-columns wins for ${selectors.join(' / ')}`).toBeTruthy()
  const out: string[] = []
  let depth = 0
  let cur = ''
  for (const ch of (v ?? '').replace(/\s+/g, ' ').trim()) {
    if (ch === '(') depth++
    if (ch === ')') depth--
    if (ch === ' ' && depth === 0) {
      if (cur) out.push(cur)
      cur = ''
    } else cur += ch
  }
  if (cur) out.push(cur)
  return out
}

const pxOf = (v: string | null) => (v == null ? 0 : Number(/(-?\d+(?:\.\d+)?)px/.exec(v)?.[1] ?? 0))
/** The controls the row renders, in order, with station control held (the desktop default,
 *  and the widest case). If this changes, the measurement above is stale. */
const EXPECTED_LABELS = ['📢', 'QRZ', 'QRZ✎', 'CL', 'HL', 'WRL', 'QSL▸', 'SAT▸', '✎', '✕']

// ── What the component actually renders ─────────────────────────────────────────────────
beforeAll(() => {
  globalThis.ResizeObserver = class {
    observe() {}
    unobserve() {}
    disconnect() {}
  } as unknown as typeof ResizeObserver
  Object.defineProperty(HTMLElement.prototype, 'offsetHeight', { configurable: true, value: 600 })
  Object.defineProperty(HTMLElement.prototype, 'offsetWidth', { configurable: true, value: 900 })
})
// Each test's Logbook unmounted when the test ends. Left mounted, it stays subscribed to the log
// answers still on their way, and one landing after this file's window is gone re-renders it
// there: "window is not defined", counted as an uncaught error against the run.
afterEach(cleanup)

/** The log the engine holds: `askLog` answers from it as the engine does (features/logAnswers.testkit). */
const engineLog = vi.hoisted(() => vi.fn())
vi.mock('./api', () => {
  const noop = () => vi.fn()
  return {
    askLog: vi.fn(async (q: LogQuestion) => (await import('./features/logAnswers.testkit')).answerAs(q, await engineLog())),
    getWavelogV2Status: vi.fn(async () => ({ localTotal: 1, remoteTotal: 1, syncedLocal: 1, syncedRemote: 1, localOnly: 0, remoteOnly: 0, lastSyncUnix: 1, intervalSeconds: 300 })),
    deleteQsoById: noop(), editQsoById: noop(), exportGeneralLog: noop(), importAdif: noop(),
    logOperators: vi.fn(() => Promise.resolve([] as string[])), exportLogForOperator: noop(),
    logActivations: vi.fn(() => Promise.resolve([])), exportLogForActivation: noop(),
    // ⚠️ NON-EMPTY, unlike the sibling Logbook suites: the satellite menu renders only
    // when the backend hands it names, and a cluster measured without it is a cluster
    // no operator sees. The names are LoTW's own (Engine::LOTW_SAT_NAMES).
    lotwSatNames: vi.fn(async () => ['AO-91', 'ARISS', 'SO-50']),
    setSatTagById: vi.fn(async () => ({})),
    saveTextToDownloads: noop(),
    logQso: noop(), markQslSentById: noop(), purgeLog: noop(), qrzLookup: noop(),
    syncLotwReport: noop(), uploadLotwReport: noop(), qrzPushQso: noop(),
    clublogPushQso: noop(), hrdlogPushQso: noop(),
  }
})
vi.mock('./toast', () => ({ pushToast: vi.fn(), withErrorToast: vi.fn() }))

async function renderRow(moreColumns = false): Promise<HTMLElement> {
  // `moreColumns` is read from localStorage on mount (#239's "More columns" toggle), so the
  // wide table is rendered by seeding it rather than by driving the chip.
  window.localStorage.setItem('nexus.logbook.moreColumns', moreColumns ? '1' : '0')
  engineLog.mockResolvedValue([
    {
      call: 'K0ABC', grid: 'EN37', band: '20m', freqMhz: 14.074, mode: 'FT8',
      rstSent: '-10', rstRcvd: '-12', name: null, qth: null, comment: null, notes: null,
      country: 'United States', whenUnix: 1_700_000_000, confirmed: false,
      awardConfirmed: false, qslRcvd: null, qslSent: null, ota: null, upload: undefined,
      // Tagged, so the satellite menu carries its removal entry too — the widest it gets.
      propMode: 'SAT', satName: 'SO-50',
    },
  ])
  const { container } = render(
    <Logbook defaultBand="20m" defaultFreqMhz={14.074} defaultMode="FT8" />,
  )
  await waitFor(() => expect(container.querySelector('.logbook-row:not(.head)')).not.toBeNull())
  // The whole cluster, the satellite menu in it. That menu is the one control a SEPARATE answer
  // draws — the backend's list of names (`lotwSatNames`) — which lands on its own schedule, so
  // the row can be on screen a moment before it. Counted then, the cluster is nine controls, not
  // the ten an operator sees.
  const satMenu = t('logbook.row.sat.aria', { call: 'K0ABC' })
  await waitFor(() =>
    expect(
      container.querySelector(`.logbook-row:not(.head) select[aria-label="${satMenu}"]`),
      'the satellite names never reached the row',
    ).not.toBeNull(),
  )
  return container.querySelector('.logbook-row:not(.head)') as HTMLElement
}

describe('the Logbook row-action menu stays compact and complete', () => {
  it('renders every former row action inside one gear menu', async () => {
    const row = await renderRow()
    const menu = row.querySelector('.log-actions-menu')
    expect(menu).not.toBeNull()
    expect(menu!.querySelector(':scope > summary svg')).not.toBeNull()
    const labels = [...menu!.querySelectorAll('.log-actions-popover .log-rowbtn')].map((el) =>
      el.tagName === 'SELECT' ? (el as HTMLSelectElement).options[0].text : (el.textContent ?? ''),
    )
    expect(labels).toEqual(EXPECTED_LABELS)
  })

  it('keeps the compact row aligned with one narrow action track', async () => {
    const row = await renderRow()
    const cells = row.querySelectorAll(':scope > .log-cell').length
    const template = tracks(['.logbook-row'])
    expect(template.length).toBe(cells)
    expect(template[template.length - 1]).toBe('42px')
  })

  it('makes the popover controls readable instead of squeezing their labels', async () => {
    const row = await renderRow()
    const controls = row.querySelectorAll('.log-actions-popover .log-rowbtn').length
    expect(controls).toBe(EXPECTED_LABELS.length)
    expect(pxOf(winner(['.log-actions-popover'], 'min-width'))).toBeGreaterThanOrEqual(160)
    expect(winner(['.log-actions-popover .log-rowbtn'], 'width')).toBe('100%')
  })

  it('keeps deletion inside the menu as the destructive action', async () => {
    const row = await renderRow()
    const danger = row.querySelector('.log-actions-popover .log-rowbtn.danger')
    expect(danger).not.toBeNull()
    expect(danger?.getAttribute('aria-label')).toContain('K0ABC')
  })
})
