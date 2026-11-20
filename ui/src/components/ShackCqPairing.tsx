import { useEffect, useState } from 'react'
import {
  exportShackCqUnassignedContacts,
  forgetShackCqPairing,
  flushShackCqContactOutbox,
  getShackCqContactOutboxStatus,
  getShackCqAgentStatus,
  getShackCqPairing,
  pairShackCq,
  retryShackCqRejectedContacts,
  startShackCqAgent,
  stopShackCqAgent,
  type ShackCqAgentStatus,
  type ShackCqPairingRecord,
  type ShackCqContactOutboxStatus,
} from '../api'

type Props = Readonly<{ mode: 'basic' | 'digi' }>

export function ShackCqPairing({ mode }: Props) {
  const [pairing, setPairing] = useState<ShackCqPairingRecord | null>(null)
  const [agent, setAgent] = useState<ShackCqAgentStatus | null>(null)
  const [loaded, setLoaded] = useState(false)
  const [open, setOpen] = useState(false)
  const [origin, setOrigin] = useState('https://shackcq.com')
  const [code, setCode] = useState('')
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState('')
  const [outbox, setOutbox] = useState<ShackCqContactOutboxStatus | null>(null)
  const [unassignedAdif, setUnassignedAdif] = useState<string | null>(null)

  useEffect(() => {
    let live = true
    void getShackCqPairing()
      .then(async (record) => {
        if (live) setPairing(record)
        if (record) return startShackCqAgent()
        return getShackCqAgentStatus()
      })
      .then((status) => { if (live) setAgent(status) })
      .catch((reason) => { if (live) setError(String(reason)) })
      .finally(() => { if (live) setLoaded(true) })
    return () => { live = false }
  }, [])

  useEffect(() => {
    if (!pairing) return
    const poll = () => {
      void getShackCqAgentStatus().then(setAgent)
      // The local journal remains readable when the website is offline. Show
      // its pending count before attempting network delivery, including after
      // a crash/relaunch where the previous UI state no longer exists.
      void getShackCqContactOutboxStatus()
        .then(setOutbox)
        .catch(() => undefined)
        .then(() => flushShackCqContactOutbox().then((status) => {
          setOutbox(status)
          if (status.delivered > 0) {
            window.dispatchEvent(new Event('shackcq-contact-outbox-delivered'))
          }
        }).catch(() => undefined))
    }
    poll()
    const timer = window.setInterval(poll, 5_000)
    return () => window.clearInterval(timer)
  }, [pairing])

  const pair = async () => {
    setBusy(true)
    setError('')
    try {
      const record = await pairShackCq(origin.trim(), code, `ShackCQ Desktop ${mode === 'basic' ? 'Basic' : 'Digi'}`)
      setPairing(record)
      setAgent(await startShackCqAgent())
      setCode('')
      setOpen(false)
      window.dispatchEvent(new Event('shackcq-pairing-changed'))
    } catch (reason) {
      setError(reason instanceof Error ? reason.message : String(reason))
    } finally {
      setBusy(false)
    }
  }

  const forget = async () => {
    setBusy(true)
    setError('')
    try {
      await stopShackCqAgent()
      await forgetShackCqPairing()
      setPairing(null)
      setAgent(null)
      setOutbox(null)
      window.dispatchEvent(new Event('shackcq-pairing-changed'))
    } catch (reason) {
      setError(reason instanceof Error ? reason.message : String(reason))
    } finally {
      setBusy(false)
    }
  }

  const exportUnassigned = async () => {
    setBusy(true)
    setError('')
    try {
      setUnassignedAdif(await exportShackCqUnassignedContacts())
    } catch (reason) {
      setError(reason instanceof Error ? reason.message : String(reason))
    } finally {
      setBusy(false)
    }
  }

  const retryRetained = async () => {
    setBusy(true)
    setError('')
    try {
      const status = await retryShackCqRejectedContacts()
      setOutbox(status)
      if (status.lastError) setError(status.lastError)
    } catch (reason) {
      setError(reason instanceof Error ? reason.message : String(reason))
    } finally {
      setBusy(false)
    }
  }

  if (!loaded) return <div className="shackcq-pairing" role="status">Checking ShackCQ pairing…</div>
  return (
    <section className={`shackcq-pairing ${pairing ? 'is-paired' : 'is-unpaired'}`} aria-label="ShackCQ Web pairing">
      <div>
        <strong>{pairing ? `Paired · ${pairing.stationLabel}` : 'ShackCQ Web is not paired'}</strong>
        <span>{pairing ? `${pairing.accountLabel} · ${agent?.connected ? 'website telemetry and control live' : agent?.lastError ? `offline · ${agent.lastError}` : 'connecting…'} · ${outbox === null ? 'checking contact delivery' : outbox.pending ? `${outbox.pending} contact${outbox.pending === 1 ? '' : 's'} pending${outbox.awaitingDestination ? ` (${outbox.awaitingDestination} awaiting destination)` : ''}` : 'contact delivery caught up'} · credentials stay in the system keychain` : 'Create a one-time Desktop pairing code on ShackCQ Web.'}</span>
      </div>
      {pairing ? (
        <button type="button" disabled={busy} onClick={() => void forget()} title="This removes the local credential. Revoke a lost or stolen device from ShackCQ Web.">Forget this Mac</button>
      ) : (
        <button type="button" onClick={() => setOpen((value) => !value)}>{open ? 'Cancel' : 'Pair'}</button>
      )}
      {open && !pairing && (
        <form onSubmit={(event) => { event.preventDefault(); void pair() }}>
          <label>Website<input value={origin} onChange={(event) => setOrigin(event.target.value)} inputMode="url" autoCapitalize="none" spellCheck={false} /></label>
          <label>12-character code<input value={code} onChange={(event) => setCode(event.target.value.toUpperCase())} autoCapitalize="characters" autoComplete="one-time-code" maxLength={16} /></label>
          <button type="submit" disabled={busy || code.replace(/[^A-Za-z0-9]/g, '').length !== 12}>{busy ? 'Pairing…' : 'Pair this Mac'}</button>
        </form>
      )}
      <button type="button" disabled={busy} onClick={() => void exportUnassigned()}>Export unassigned contacts</button>
      {unassignedAdif !== null && <div>
        <p>These contacts have no assigned logbook. Copy this ADIF for recovery; exporting does not assign, synchronize or delete them.</p>
        <label>Unassigned contacts ADIF<textarea readOnly value={unassignedAdif} rows={5} onFocus={(event) => event.target.select()} /></label>
        <button type="button" onClick={() => setUnassignedAdif(null)}>Close export</button>
      </div>}
      {error && <p role="alert">ShackCQ operation unavailable · {error}</p>}
      {Boolean(outbox?.rejected) && <>
        <p role="alert">{outbox!.rejected} contact(s) need recovery. Original contacts and failure reasons are retained; automatic retry has stopped for these items.</p>
        <p>After restoring access, retry contacts refused for access reasons. Only the original pairing and unchanged logbook destination can receive them.</p>
        <button type="button" disabled={busy || !pairing} onClick={() => void retryRetained()}>Retry retained contacts</button>
      </>}
    </section>
  )
}
