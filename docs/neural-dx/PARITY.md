# Neural DX parity ledger

Upstream authority: [`F1SMV/Neural-DX-Watcher`](https://github.com/F1SMV/Neural-DX-Watcher),
audited at commit `150587008a40474045ad3986fc4d7ce484deab1c` (v12.7). The machine-readable
pin and complete audited Flask route inventory live in `upstream.json`.

## Integration doctrine

Neural DX is a native RD-Nexus section backed by the complete pinned upstream Flask application as
a private loopback-only companion. The companion is supervised and packaged by Nexus; it is not a
separately installed service. Its duplicate cluster and WSJT-X collectors are disabled in embedded
mode, while Nexus supplies station identity, shared spots and canonical logbook state through an
authenticated bridge. Original upstream collectors and screens that Nexus does not replace remain
present. This keeps one callsign/grid identity, one source of QSO truth and one transmit-safety
boundary across macOS, Windows and Linux packages without dropping upstream capability.

The upstream source is pinned as `third_party/neural-dx-watcher`. Upstream describes its code as
MIT in the README but currently ships no licence file and no per-file licence headers. The
RD-Nexus maintainer has attested to a project-specific permission grant from the original developer
for ShackCQ.com and related software projects. `SOURCE_PERMISSION.md` records that provenance;
attribution and the exact source pin are retained.

## Capability mapping

| Neural DX Watcher capability | RD-Nexus integration authority | State |
|---|---|---|
| Dashboard, live spots, surge and band activity | Shared `getAllSpots` collection; Neural DX live summary | Integrated |
| DXCC Hunt, continuous score, top target, secondary targets | Native `scoreHuntTargets`; Nexus need priorities, worked state, distance, split and corroboration | Integrated |
| Watchlist and interesting-call tracking | Bounded durable local watchlist (100 calls) | Integrated |
| AI recent activity, hourly openings, band trends, maturity | Bounded 30-day/5,000-sample local analytics fed by the shared spot stream | Integrated |
| Propagation, solar, WSPR, beacons, modeled HF paths | Native Connect/propagation service and screen | Integrated by shared authority |
| Weather, lightning and RF correlation | Native Connect/weather intelligence | Integrated by shared authority |
| Satellites, TLE, passes, SatNOGS frequencies, rotator controls | Native Satellites service and screen | Integrated by shared authority |
| DX briefing and active expeditions | Native DXpeditions service and screen; need tags surface expedition priority | Integrated by shared authority |
| LoTW/QSO knowledge and worked status | Nexus canonical logbook and its `workedAgoSecs` enrichment | Integrated; WaveLog task will remain the sync authority |
| WSJT-X spots | Nexus station ingestion and unified spot collector | Integrated by shared authority |
| Complete original dashboard, maps, world view, AI insight, hunt, briefing, weather and satellite pages | Full pinned upstream application embedded in the Watcher tab | Integrated |
| Original API and data route surface | Machine-audited inventory of every v12.7 Flask route; the complete app is packaged, not a partial reimplementation | Integrated |
| Posting a DX spot from the upstream UI | Authenticated action queue into Nexus `post_spot`, retaining Nexus validation and cluster ownership | Integrated |
| Cross-platform application packaging | Pinned upstream source plus a Python 3.10+ self-contained sidecar built per target; no system Python or Pi-only startup script is required | Integrated; artifact-presence gates cover DMG, NSIS, deb and AppImage workflows |
| Upstream additions | Pin, route inventory and `scripts/check-neural-dx-upstream.mjs` | Integrated; update is deliberately reviewed, never auto-imported |
| VOACAP 8×24 matrix and free-form target resolver | Original upstream routes and UI remain available; the native Propagation tab additionally links to Nexus modeling | Integrated |
| REST Countries enrichment (capital, population, timezone) | Original upstream enrichment remains available; Nexus entity/grid data also feeds the shared spot snapshot | Integrated |
| Ntfy test/status and mail notification transport | Original upstream ntfy configuration, status and test routes remain available | Integrated |
| ARRL/ng3k RSS payload presentation | Original upstream briefing/feed routes remain available; the native Briefing tab also links to Nexus DXpeditions | Integrated |

“Integrated by shared authority” means Nexus supplies the capability without a second collector or
database. “Integrated” means the code and package path are present; it does not claim authenticated
LoTW, live-provider, physical-radio, rotator, signing/notarization or on-air acceptance. Those
operational evidence layers remain separate gates.

## Updating from upstream

Run:

```sh
node scripts/check-neural-dx-upstream.mjs --source /path/to/Neural-DX-Watcher
```

The command compares the remote branch SHA and the complete Flask route surface. Exit `2` means
the API surface changed; exit `3` means the source changed without a route change. Review the
upstream diff, update this ledger and the pin together, add regression tests, and only then advance
the recorded commit. Keep the permission record, attribution and source pin together.
