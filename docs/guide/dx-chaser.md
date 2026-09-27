# DX Chaser

DX Chaser brings the reviewed behavior of MSHV Auto DX Chaser into Nexus. It
ranks locally decoded FT8 and FT4 stations, follows band activity and worked
status, and can automate guarded chases through Nexus's existing radio and
sequencer. Open it from **DX Chaser** in the left rail.

The safety boundary is strict: an external RBN, PSK Reporter or DX-cluster spot
may nominate a band or station for local verification, but it can never become
a call target on its own. Only a fresh local decode can do that. DX Chaser also
cannot enable transmit; the operator must separately enable Nexus transmit
authority before arming automation.

## Before arming

1. Configure the station identity, logbook, radio, decoder and desired spot
   feeds in Nexus.
2. Open **DX Chaser → Settings** and enable the service, desired bands and
   evidence sources. Save the settings.
3. Confirm that the header shows the expected source health and that
   **Transmit authority** reflects the intended Nexus state.
4. Review **Ranked targets**. A station must have a recent local decode before
   it can be selected for a call.
5. Choose **Arm automation** only when the station is ready. **Emergency stop**
   is always available and disarms the chaser as well as stopping its current
   attempt.

Arming DX Chaser does not replace or raise the main Enable TX latch. A stop,
stale safety generation or failed radio action invalidates the pending attempt;
later re-enabling transmit cannot revive it.

## Workspace views

- **Ranked targets** shows locally decoded candidates, need flags, score,
  signal, repeat count and corroborating spotters.
- **DXCC matrix** combines canonical Nexus logbook history with read-only live
  spot evidence. Search or filter entities and inspect worked slots across the
  supported bands without triggering a tune or call.
- **Band intelligence** shows activity scores and completed-period history used
  by automatic band selection.
- **Decode cache** lists the bounded local-decode history. Network spots are
  evidence, not entries in this local-call authority.
- **Activity log** records selection, band, completion and stop decisions.
- **Statistics** separates the current session from durable all-time counters.
- **Settings** controls targeting, source participation, bands, dwell and
  scoring rules, slot selection and optional Twilio notifications.

## Band changes and external evidence

When automatic band changes are enabled, DX Chaser uses current activity,
needed spots, time exclusions, dwell, hysteresis and recent decode performance
to choose among enabled bands. A cross-band external nomination opens a bounded
local-verification window. If the nominated station is not decoded locally,
the chaser cannot call it and the temporary decision expires.

Cluster, RBN and PSK Reporter connections remain centrally owned by Nexus. The
DX Chaser source switches only decide which already-configured feeds may
contribute evidence. Contest unassisted mode and disabled feeds continue to
override those switches.

## Worked status and persistence

The Nexus logbook is the only worked-DXCC, band and grid authority. Matching
fresh logbook records complete an active attempt; unrelated or replayed records
do not. Settings, cache history and all-time statistics survive restarts using
versioned local state. Twilio credentials are stored in the operating-system
credential vault rather than the settings file.

## Twilio notifications

Notifications can be disabled, sent for each transmission, or limited to
completed QSOs. Enter the account SID, auth token and E.164 from/to numbers in
**DX Chaser → Settings**, store them, then use **Send test**. Rate limits use a
bounded retry policy. An ambiguous transport failure is not retried
automatically because doing so could duplicate a message.

A successful local test proves the client path only. Actual delivery still
requires a valid Twilio account, permitted numbers, provider acceptance and an
account-bound receipt.

## Honest limits

Automated tests and a packaged app can prove the decision engine, persistence,
UI wiring and safety guards. They do not prove a physical-radio QSY or contact,
live provider feeds, Twilio delivery, platform signing/notarization, or a
Windows/Linux runtime. Those require their corresponding hardware, accounts
and release environments.

## Related guides

- [Operate — FT8/FT4 digital](operate-digital.md) covers the decoder and guarded
  digital sequencer DX Chaser shares.
- [Needed DX](needed-dx.md) explains Nexus need scoring and evidence.
- [Spots](spots.md) shows the shared cluster and RBN traffic.
- [Logbook & QSL](logbook-qsl.md) documents the canonical worked-status source.
- [Settings reference](settings-reference.md) covers station, radio and shared
  feed configuration.
