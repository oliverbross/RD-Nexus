# Neural DX parity ledger

Upstream authority: [`F1SMV/Neural-DX-Watcher`](https://github.com/F1SMV/Neural-DX-Watcher),
audited at commit `150587008a40474045ad3986fc4d7ce484deab1c` (v12.7). The machine-readable
pin and complete audited Flask route inventory live in `upstream.json`.

## Integration doctrine

Neural DX is a native RD-Nexus section backed by the complete pinned upstream Flask application as
a private loopback-only companion. The companion is supervised and packaged by Nexus; it is not a
separately installed service. Its original cluster and WSJT-X collectors remain available, while
Nexus additionally supplies station identity, shared spots and canonical logbook state through an
authenticated bridge. Station identity and the bounded log fold travel directly from native Rust
to the child; the webview can supply only spots and cannot assert station or QSO truth. Shared spots are tagged and merged into an expanded buffer rather than
replacing upstream spots; Nexus QSO sets augment worked/confirmed sets without changing LoTW
provider login/sync provenance or double-counting provider totals. In embedded mode every original
spot-post gesture is routed exclusively through Nexus's validated cluster authority. A durable
idempotency journal and explicit delivery-unknown reconciliation prevent automatic duplicate
transmission across acknowledgement failure or process restart. This keeps one default identity
and transmit-safety boundary across macOS, Windows and Linux packages without dropping upstream
receive-side inputs or capability.

The upstream source is pinned as `third_party/neural-dx-watcher`. Upstream describes its code as
MIT in the README but currently ships no licence file and no per-file licence headers. The
RD-Nexus maintainer has attested to a project-specific permission grant from the original developer
for ShackCQ.com and related software projects. `SOURCE_PERMISSION.md` records that provenance and
the `BLOCKED_PENDING_LICENSE_EVIDENCE` public-release gate; attribution and the exact source pin
are retained.

## Capability mapping

| Neural DX Watcher capability | RD-Nexus integration authority | State |
|---|---|---|
| Dashboard, live spots, surge and band activity | Shared `getAllSpots` collection; Neural DX live summary | Integrated |
| DXCC Hunt, continuous score, top target, secondary targets | Native `scoreHuntTargets`; Nexus need priorities, worked state, distance, split and corroboration | Integrated |
| Watchlist and interesting-call tracking | Bounded durable local watchlist (100 calls) | Integrated |
| AI recent activity, hourly openings, band trends, maturity | Bounded 30-day/5,000-sample local analytics fed by the shared spot stream | Integrated |
| Propagation, solar, WSPR, beacons, modeled HF paths | Original routes/page retained; native Connect/propagation is an additional shared view | Integrated; live providers remain an evidence gate |
| Weather, lightning and RF correlation | Original routes/page retained; native Connect/weather is an additional shared view | Integrated; live providers remain an evidence gate |
| Satellites, TLE, passes, SatNOGS frequencies, rotator controls | Original routes/page retained; native Satellites is an additional shared view | Integrated; hardware/provider operation remains an evidence gate |
| DX briefing and active expeditions | Original routes/page retained; native DXpeditions is an additional shared view | Integrated; live feeds remain an evidence gate |
| LoTW/QSO knowledge and worked status | One cached native-to-child log fold supplies bounded worked/confirmed sets; Nexus knowledge remains usable when the separate LoTW provider is logged out, while its session/counters remain independently authoritative | Integrated; WaveLog task will remain the sync authority |
| WSJT-X spots | Original UDP collector retained; Nexus station ingestion is an additional shared input | Integrated |
| Complete original dashboard, maps, world view, AI insight, hunt, briefing, weather and satellite pages | Full pinned upstream application embedded in the Watcher tab | Integrated |
| Original API and data route surface | Machine-audited inventory of every v12.7 Flask route; the complete app is packaged, not a partial reimplementation | Integrated |
| Posting a DX spot from the upstream UI | Persistent authenticated child queue into a durable native action-id journal, then Nexus `post_spot`; interrupted delivery is never auto-retried and requires explicit operator resolution | Integrated |
| Cross-platform application packaging | Pinned upstream source plus a Python 3.10+ self-contained sidecar built per target; no system Python or Pi-only startup script is required | Implemented; public artifacts blocked pending licence evidence; non-macOS runtime still needs CI execution |
| Upstream additions | Pin, route inventory and `scripts/check-neural-dx-upstream.mjs` | Integrated; update is deliberately reviewed, never auto-imported |
| Hosted Remote | Desktop companion commands are intentionally absent from the restricted Remote transport | Truthfully hidden from Remote; desktop integration is unchanged |
| VOACAP 8×24 matrix and free-form target resolver | Original upstream routes and UI remain available; the native Propagation tab additionally links to Nexus modeling | Integrated |
| REST Countries enrichment (capital, population, timezone) | Original upstream enrichment remains available; Nexus entity/grid data also feeds the shared spot snapshot | Integrated |
| Ntfy test/status and mail notification transport | Original upstream ntfy configuration, status and test routes remain available | Integrated |
| ARRL/ng3k RSS payload presentation | Original upstream briefing/feed routes remain available; the native Briefing tab also links to Nexus DXpeditions | Integrated |

“Integrated” means the original route/page and package path are present, with any native Nexus view
listed separately as an augmentation; it does not claim authenticated
LoTW, live-provider, physical-radio, rotator, signing/notarization or on-air acceptance. Those
operational evidence layers remain separate gates.

## Exact surface inventory

`upstream.json` is the machine-readable parity ledger. It records all 38 tracked upstream files,
all 91 distinct Flask paths, all nine templates, both static stylesheets, runtime modules,
diagnostics/tests, documentation/history files, persistence formats, external inputs, outputs and
the six RD-Nexus overlays. The checker compares both the complete Git file list and route list;
an added non-route file is therefore just as visible as a new endpoint.

| Upstream view/template | Embedded behavior | Native Nexus augmentation |
|---|---|---|
| `index.html` | Exact dashboard, live bands, spots, surge and controls | Shared Nexus spot/QSO snapshots |
| `map.html` | Exact geographic map and spot APIs | Nexus station identity and shared spots |
| `world.html` | Exact world-event surface | None required |
| `ai_insight.html` | Exact analytics/predictor/meta UI | Shared spots feed the same predictor and analytics modules |
| `apercu_voacap.html` | Exact VOACAP matrix/target UI and adapter contract | Native Propagation tab is also available |
| `briefing.html` | Exact RSS/DX briefing behavior | Native DXpeditions tab is also available |
| `hunt.html` | Exact upstream DXCC/LoTW hunt | Canonical Nexus QSOs are merged into LoTW state; native Hunt adds Nexus need scoring |
| `weather.html` | Exact WSPR, beacon, lightning, NOAA and correlation UI | Native Weather/Connect tab is also available |
| `satellites.html` | Exact TLE, catalog, pass, footprint and frequency UI | Native Satellites/rotator tab is also available |

The two upstream CSS assets are copied byte-for-byte into the one-file executable. Python imports
bundle the upstream analytics, country metadata, DXCC hunt/resolver, ntfy, predictor and VOACAP
modules. `tools/log_meta_analyzer.py` is explicitly hidden-imported and its route is replaced by an
equivalent in-process call because a packaged application must not depend on a user's `python3`.
Pillow is an explicit package dependency so `/api/forecast/heatmap.png` does not lose its optional
runtime import. Maintenance patch scripts, tests, deployment diagnostics, docs, screenshot,
historical source copies and `start.sh` remain in the pinned source submodule; they are audit/source
artifacts rather than application runtime resources. `start.sh` is deliberately superseded by the
cross-platform Tauri supervisor.

## Settings, persistence and recovery

Upstream defaults and mutable formats remain upstream-owned: callsign/QRA/GPS config, UI config,
watchlist/activity, briefing sources, beacon cache, satellite config, LoTW cache, API token,
predictor/analytics/ntfy SQLite databases, CTY data, meta summary and rotating log. They live under
Nexus's per-user local-data directory, so upgrades retain them and different OS users do not share
credentials. The original callsign/QRA controls remain functional, but the next Nexus sync restores
the station identity as the default authority.

The companion asks the kernel for its own loopback port and atomically reports the bound port and
identity, avoiding a reserve-then-drop race between parallel Nexus instances. The supervisor uses
an OS-cryptographic 256-bit per-process bridge token, requires an authenticated identity/version
status response before declaring readiness, launches with no terminal window on Windows, exposes
starting/ready/unavailable state, detects child exit, offers restart, and kills/reaps the child when
Nexus closes. Upstream HTTP pages keep their own persisted API token; Nexus-only routes use
constant-time token comparison and reject missing/wrong tokens. Requests, responses, collection
sizes, strings, spot-action ranges and queue depth are bounded; replayed spot commands are
deduplicated by stable/full identity using station receive time, DX/spotter, frequency, band, mode
and comment, with a bounded six-hour LRU. The embedded meta-analysis route
always requires a cryptographic token and cannot be pointed at an arbitrary filesystem path. The
iframe sandbox excludes top-navigation. Pending child actions survive supervised restarts; native
execution receipts survive Nexus restarts and make repeated action ids idempotent. The token is
neither written to the ready file nor included in error messages.

## Runtime and package evidence

| Layer | Evidence | Current result |
|---|---|---|
| Pin and drift | live remote SHA; exact file and route inventories; positive/negative diff tests | Executed locally at the recorded pin |
| Upstream unit behavior | `test_analytics.py`, `test_country_meta.py`, `test_dxcc_hunt.py` | Passed locally in the packaged Python 3.12 environment |
| UI/source | production UI build, focused Neural DX/i18n tests, full suite plus isolated retry of unrelated failures | Production build passed; 224 focused tests passed; full suite exposed one pre-existing Satellite Remote failure outside this change |
| Rust host | action-journal fault/restart tests plus 67k native log-summary benchmark with `radio,custom-protocol` | 5 focused tests passed; 67,000 QSOs folded to 627 bytes in 962 ms on the final local run |
| Packaged companion | Python 3.12 PyInstaller build/self-test; authenticated sync; logged-out consumer path; action validation/deduplication; embedded meta analysis | Passed locally on Apple Silicon |
| Bridge composition | overlap/deletion/restart, provider provenance, same-second production spot identity, bounded LRU, repeated sync, saturation/restart/no-loss action queue, meta-route auth/path bounds | 7 bridge regression tests passed locally |
| macOS bundle | resource presence, Developer ID signing, hardened runtime, notarization and DMG smoke | Workflow gates added; signing/notarization require release secrets/CI |
| Windows bundle | native Windows sidecar job plus NSIS content gate | Workflow gates added; runtime needs Windows CI/artifact |
| Linux/Pi bundles | native build in `build-linux.sh`, deb/AppImage/Pi payload gates | Workflow gates added; runtime needs Linux/Pi CI/artifact |
| Live providers/radio | LoTW account, VOACAP engine, cluster, WSJT-X, ntfy, satellite/rotator and RF operation | Not established by local build evidence |

## Updating from upstream

Run:

```sh
node scripts/check-neural-dx-upstream.mjs --source /path/to/Neural-DX-Watcher
```

The command compares the remote branch SHA, complete tracked-file inventory and complete Flask
route surface. Exit `2` means a file or route changed; exit `3` means the remote commit changed
without an inventory change. Review the
upstream diff, update this ledger and the pin together, add regression tests, and only then advance
the recorded commit. Keep the permission record, attribution and source pin together. Upstream's
`/api/check_update` remains visibility-only in an immutable companion; updates are never allowed to
overwrite the packaged binary in place.
