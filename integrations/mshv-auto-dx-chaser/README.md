# MSHV Auto DX Chaser integration

Nexus integrates the behavior of
[`kd9taw/mshv-auto-dx-chaser`](https://github.com/kd9taw/mshv-auto-dx-chaser),
pinned in `UPSTREAM.toml`. It is a behavioral port, not a second embedded MSHV
application. The portable Rust engine is compiled into every Nexus desktop binary and
the Tauri adapter reuses Nexus's existing decoded-message, spot, logbook, radio,
credential and packaging authorities.

This boundary is safety-critical. External spots may rank or trigger a QSY for local
verification, but only a current local decode can become a call target. DX Chaser never
raises Nexus's Enable-TX latch. Band and frequency changes use the same Engine path as
the rest of Nexus, so privilege, identity, watchdog and radio-routing gates remain in
force.

## Upstream refresh

Run:

```sh
scripts/sync-mshv-auto-dx-chaser.sh --check
scripts/sync-mshv-auto-dx-chaser.sh --stage /absolute/empty/review-directory
```

`--check` fails when upstream has moved beyond the reviewed pin. `--stage` fetches a
detached review copy without changing Nexus. Review its commits and every changed
AutoDxer, AutoBand, AutoDxSettings, DxccChaser and DxccTracker file, port behavioral
changes with tests, then update the pin. The script never treats a new upstream commit
as integrated merely because it was downloaded.

`UPSTREAM.toml` is the single reviewed-revision manifest used by the freshness check
and package-content gate. The weekly `MSHV Auto DX Chaser upstream watch` workflow runs
the same check, so a new upstream revision becomes a visible failing review request.
After reviewing and porting it, update the manifest and the compiled
`dx_chaser::UPSTREAM_REVISION`; the package gate then proves that the reviewed revision
and DX Chaser UI are present together in each shipped executable.

## Parity map

| Upstream subsystem | Nexus integration |
|---|---|
| `HvAutoDxer` target cache, filters, scoring, rarity, persistence and engage lock | `crates/dx-chaser` |
| RBN, PSK Reporter and DX-cluster collectors | Existing `tempo-net` / propagation feeds, translated by `src-tauri/src/dx_chaser.rs` |
| `HvAutoBand` activity, band choice, cross-band verification and slot selection | `crates/dx-chaser`, executed through `tempo-app::Engine` |
| DXCC tracker, ADIF import and worked/new facts | Existing Nexus SQLite logbook and `DecodeRow` need flags |
| Auto DX settings and statistics panels | `ui/src/components/DxChaserView.tsx` |
| Twilio alerts | Existing `reqwest` stack plus OS-keychain-only credentials in the adapter |
| Linux/Windows application build | Compiled into the normal Nexus Tauri binary |
| macOS application build | Same Rust/Tauri implementation; no Qt-only upstream dependency |

The executable contains no independent CAT/PTT controller, decoder, logbook database or
spot network client from MSHV. Duplicating those would make state disagree and would
create a second transmit path outside Nexus's safety gates.
