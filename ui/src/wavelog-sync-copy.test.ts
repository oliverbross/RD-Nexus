import { describe, expect, it } from 'vitest'
import { DE } from './i18n/de'
import { EN } from './i18n/en'
import { ES } from './i18n/es'
import { FR } from './i18n/fr'
import { JA } from './i18n/ja'

const SUPPORTED_LOCAL_CHANGES = [
  { catalog: EN, wording: 'supported local changes' },
  { catalog: DE, wording: 'unterstützte lokale Änderungen' },
  { catalog: ES, wording: 'cambios locales admitidos' },
  { catalog: FR, wording: 'modifications locales prises en charge' },
  { catalog: JA, wording: '対応するローカル変更' },
]

describe('Wavelog sync scope copy', () => {
  it.each(SUPPORTED_LOCAL_CHANGES)(
    'qualifies the incremental PATCH scope in every locale',
    ({ catalog, wording }) => {
      expect(catalog['logbook.sync.cadence']).toContain(wording)
      expect(catalog['settings.confirmations.cloudlog.fullAudit.hint']).toContain(wording)
    },
  )
})
