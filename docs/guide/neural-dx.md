# Neural DX

Neural DX brings the complete Neural DX Watcher workspace into the Nexus desktop
app. Open it from **Neural DX** in the left rail, immediately below **Spots**.
It keeps the original Watcher, propagation, weather, satellite, notification and
briefing capabilities, while Nexus supplies the station identity, live spot set,
logbook knowledge and the only path to radio control.

Neural DX is a desktop-only section. It is deliberately absent from Hosted
Remote because that restricted client cannot supervise the local companion or
use Nexus's native command boundary.

## The tour

The tab strip divides the workspace into seven views:

- **Watcher** embeds the original Neural DX Watcher application. Its dashboards,
  filters, analytics, watchlist, activity, settings, LoTW tools, VOACAP tools,
  clusters, WSJT-X input, ntfy notifications and upstream pages remain present.
- **Hunt** ranks the current Nexus spots using Neural DX scoring and shows the
  strongest target first. Select a row to inspect it, watch it, tune it or ask
  Nexus to work it.
- **AI Insight** summarizes the current targets, score distribution, bands and
  time-of-day activity from the shared spot set.
- **Propagation**, **Weather**, **Satellites** and **Briefing** open the matching
  original Neural DX pages inside the same workspace.

The status at the top reports whether the bundled companion is starting, ready
or unavailable. **Restart** appears when the companion needs recovery. Nexus
starts and stops it with the app; there is no separate server for an operator to
install or launch.

## Core workflows

### Inspect and work a target

1. Open **Neural DX ▸ Hunt**.
2. Select a target in the ranked list. The large card shows its callsign,
   entity, band, mode, frequency, score and the reasons for that score.
3. Select the callsign or the tune action to tune through Nexus's normal radio
   authority.
4. Select **Work** to hand the target to the appropriate Nexus operating
   cockpit. Nexus still applies its normal transmit interlocks; Neural DX never
   keys the radio directly.

If an action reached the radio boundary but Nexus could not prove its final
result, the screen reports **delivery unknown**. Inspect the station before
choosing **Mark delivered** or **Retry**. This avoids silently repeating a radio
operation.

### Watch a callsign

1. Select a target in **Hunt** or open the original **Watcher** tab.
2. Add or remove the callsign with the Watch control.
3. The original watchlist and activity history persist in the Neural DX data
   store. New matching Nexus spots are synchronized to the companion.

### Use the original workspace

1. Open **Watcher** for the original home dashboard, or choose one of the four
   original tool tabs.
2. Configure provider-specific features in the original settings screens. A
   LoTW login, cluster host, VOACAP engine, WSJT-X feed, weather service, ntfy
   endpoint or satellite source still needs its own valid configuration and
   network access.
3. Return to **Hunt** when you want Nexus-native spot and radio actions.

## Shared data and authority

Nexus remains the authority for the station profile, radio, transmit safety and
canonical logbook. The browser surface sends only bounded spot records to the
native host. The native host reads the station settings and a compact worked-
before summary from Nexus itself, then passes those to the companion over an
authenticated loopback bridge. Web content cannot supply or replace station or
logbook truth.

The original Neural DX LoTW session is preserved for original LoTW workflows,
but it is not the source of Nexus logbook knowledge. Hunt can identify a worked
callsign from the Nexus logbook while Neural DX is logged out of LoTW. Neural DX
collectors can coexist with Nexus's spot feeds; synchronized spots are
deduplicated using the complete spot identity, including receive time, spotter,
frequency, band, mode and comment.

Radio-facing requests from the original workspace enter a durable bounded queue
and are executed only through Nexus's native authority. At 100 pending actions,
the bridge rejects a new request instead of discarding an older one. Prepared,
transmitting, successful and uncertain outcomes are journalled across restarts
so replay does not turn an ambiguous operation into an accidental duplicate.

## Persistence, privacy and recovery

The bundled companion listens only on a kernel-selected loopback port. Each run
uses a fresh operating-system-generated authentication secret and an
authenticated readiness handshake. Nexus stops and reaps the process on exit;
parallel Nexus instances receive separate ports and identities.

Original Neural DX settings, watch activity, briefing sources, provider caches,
tokens, analytics and logs live in Nexus's per-user application-data directory.
They survive an app upgrade and are not shared between operating-system users.
Use **Restart** after an unexpected companion exit. A restart retains queued
actions and completed-action receipts.

## Packaging and updates

The companion is built into the Nexus desktop packages for macOS, Windows,
Linux and Raspberry Pi. It is pinned to a reviewed Neural DX Watcher revision;
the original update check is informational and cannot replace files inside an
installed package. Maintainers use the recorded source pin, full tracked-file
inventory and full route inventory to detect future upstream additions. A pin
only advances after the diff, parity ledger and regression coverage are updated.

Public packages are fail-closed until the repository contains written evidence
that permits modification and redistribution under terms compatible with the
Nexus release. Permission supplied privately can authorize private builds, but
is not silently treated as public-license evidence.

## Honest limits

Build and automated-test success do not prove a live LoTW account, VOACAP
installation, cluster, WSJT-X station, ntfy endpoint, weather service,
satellite/rotator, radio, signing identity or notarized public installer. Those
remain account-, network-, hardware- or release-environment checks. Neural DX
does not bypass Nexus transmit interlocks, make Hosted Remote a local desktop,
auto-install upstream updates, or turn predicted propagation into an on-air
contact.

## Related guides

- [Spots](spots.md) explains the native live spot table that feeds Hunt.
- [Logbook & QSL](logbook-qsl.md) explains Nexus's canonical contact and
  confirmation records.
- [Connect — map + propagation](connect.md) covers Nexus-native mapping and
  propagation views.
- [Satellites](satellites.md) covers Nexus-native pass and rotator workflows.
- [Settings reference](settings-reference.md) covers station, radio, feed and
  feature configuration.
