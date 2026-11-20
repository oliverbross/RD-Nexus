# ShackCQ Desktop Nexus implementation and evidence

## Result

`NOT_COMPLETE`. This is a coherent, packaged first slice based on the complete pinned Nexus product, not the earlier ShackCQ adapter. Static package verification passed; packaged operator workflows, external services and hardware remain unexercised.

## Immutable baseline and source identity

- Upstream: Nexus v1.14.0 at `12efe3d2970729465add40d62cecd76bbaf5b63f`.
- Release evidence supplied by the parent task: annotated tag object `4146ee1f…`, peeled commit exactly as above, published `2026-09-20T20:48:49Z`.
- Product worktree: `/Users/oliver/Documents/Projects/RigWeave/shackcq-desktop-nexus-20260925`.
- Product branch: `codex/shackcq-nexus-product-20260925`.
- Web worktree: `/Users/oliver/Documents/Projects/RigWeave/desktop-web-ux-web-20260924`, HEAD `98d56a2ac0a03a40028fd02309abcec191c439b7`, dirty with this contract slice and pre-existing generated/untracked files.
- Prior recorded frontend content digest: `70a89aa334ba254158be9c96a862b8a891d282c6c50c7528608d5eb5bf7134b9`. It predates the new contract files and is not an artifact manifest for this dirty slice.
- Installed app preserved and not launched or overwritten: `/Applications/ShackCQ Desktop.app`, bundle `online.shackcq.desktop`, version `0.2.2`.

## Architecture

Nexus is the desktop product base. Its UI, application state, radio/multi-radio arbitration, Hamlib/native control, rotctld/satellite tracking, codecs, scheduling, audio, local logbook and provider integrations remain authoritative. ShackCQ adds a narrow website boundary; it does not introduce another modem, state machine, radio/rotator controller, PCM owner or logbook master.

The review distribution uses:

- product `ShackCQ Desktop Nexus Review`;
- bundle identifier `app.shackcq.desktop.nexusreview`;
- binary `shackcq-desktop-nexus-review`;
- isolated profile/data/gallery/recording and keychain/vault namespaces;
- no Nexus updater public key or endpoint, and updater artifacts disabled;
- GPL-3.0-only plus retained upstream copyright, COPYING and NOTICE.

The desktop connector accepts only HTTPS (or HTTP loopback for fixtures), disables redirects, bounds token/identity/streamed-response values and resolves bearer material natively from an opaque keychain alias; raw machine tokens do not cross Tauri IPC. Its declared scopes are `station.observe`, `contact.ingress`, `configuration.read` and `logbook.read`. `SHACKCQ_DIGI_TX_ENABLED` is hard-coded `false`, and the application command that arms Nexus transmit fails explicitly while that gate is false. The complete encoders/sequencers remain compiled, but the review candidate cannot arm the universal Nexus TX latch. There are no PCM, CAT, Tune, PTT or rotator methods in the connector.

The app now has two product modes. **Basic** is the fail-closed default and exposes Connect, Needed and Settings for radio/rotator setup while hiding the digital suite and TX chrome. **Digi** exposes the complete Nexus suite. Entering Digi starts a bounded pull of the Web-selected canonical logbook and repeats it every five minutes; the returned ADIF is imported through Nexus's existing importer and duplicate QSOs are skipped. The configuration follows the Web-selected station/logbook authority rather than adding a second desktop master.

The connector commands are registered and unit-tested, and Digi logbook pull has visible sync status. Station-state publishing, completed-contact push and operator enrollment/token issuance are **not yet wired** to authoritative Nexus events or a pairing UI. Without connector metadata plus a natively stored credential alias, sync fails closed as unpaired. Basic-mode radio/rotator-to-website telemetry therefore remains a real integration gate rather than a claimed working feature.

## Implemented slice

- `ui/src/productMode.ts`, `ModeNav.tsx`, `App.tsx`: persisted Basic/Digi selection, Basic-safe navigation and automatic stop/fallback when leaving Digi.
- `ui/src/useShackCqLogbookSync.ts`: Digi-entry plus five-minute selected-logbook synchronization with visible inactive/unpaired/syncing/ready/error state.
- `src-tauri/src/shackcq_connector.rs`: capabilities, bounded configuration/logbook retrieval, Nexus ADIF import and the earlier station/contact delivery commands; native keychain lookup keeps raw bearer credentials out of the webview.
- `src-tauri/src/lib.rs`: five Tauri commands registered.
- `src-tauri/tauri.conf.json`, `Cargo.toml`, profile/data/vault code: isolated review identity and paths.
- `packages/contract/src/nexusDesktop.ts` in ShackCQ Web: strict shared protocol, grant, configuration and full-QSO ingress parsing.
- `services/hosted/src/cloudServer.ts`: authenticated desktop configuration and canonical selected-logbook ADIF endpoints using existing agent credentials and station-authority checks.
- ShackCQ Web application/public surfaces: Digi navigation, route, settings and entry links removed; desktop is now the Digi destination.

## Verification performed

- `npm test --workspace @shackcq/contract`: 42 passed, 0 failed.
- `npm test -w @shackcq/web`: 222 passed, 0 failed after a production Web build and prerender.
- `npm run typecheck -w @shackcq/hosted-service`: passed.
- `cargo test --lib --features radio shackcq_connector::tests`: 5 passed, 0 failed; compilation only, with no process launch or hardware use.
- `ui/src/productMode.test.ts`: 3 passed, 0 failed as part of the desktop UI test run.
- `cargo test --lib --features radio profile_dir_name`: 1 passed.
- `cargo test --lib --features radio sstv_scope`: 2 passed.
- `cargo fmt --all`: completed.
- `npm ci && npm run build` in `ui/`: production frontend built successfully; npm reported 4 moderate and 5 high dependency audit findings, which were not auto-mutated.
- DeepCW release assets were staged from upstream release `deepcw-model-v1`: `model.onnx` SHA-256 `a7b3b4aa2610d3e88b8c7d4420eceee31be2d0ae595b3413264334f3ffb1eab5`; `model.onnx.json` SHA-256 `b4342157b90229ee7380e165f3d8036179b80d1e6ae02f2557388b7fcd558c01`.
- `tauri-cli 2.11.5` was installed from the pinned v2 major for the repository's official packaging lane.
- The first normal release compile failed closed at `build.rs:28` while that model was absent. A documented model-less compile was started only as diagnostic evidence, then terminated after the verified full asset was staged; it is not a candidate artifact.
- The official Tauri lane produced the isolated app. Its initial linker-only ad-hoc signature used a Rust-derived identifier and failed strict resource verification; the bundle was re-signed ad hoc with `app.shackcq.desktop.nexusreview`. `codesign --verify --deep --strict` then passed. No Developer ID, signing identity, notarization or stapling was used.
- Stable DMG: `src-tauri/target/release/bundle/macos/ShackCQ_Desktop_Nexus_Review_1.14.0-review.2_aarch64.dmg`, SHA-256 `66e2b2f9550d138c05b39f83e571a4f0e9da033d35d3c4e5932ea2d064b3217e`.
- The DMG passed image inspection, read-only mounting, mounted-bundle identity/version checks and strict/deep code-signature verification. Gatekeeper correctly rejects it because it has no Developer ID signature. The app was not launched or installed.
- A no-`radio` focused build was attempted and failed on 21 upstream unconditional `tempo_audio` references. Making the protocol crate non-optional reduced this to two unconditional `tempo_audio::service` references, but that exploratory Cargo edit was reverted; the full Nexus product's normal `radio` feature lane remains authoritative.

## Preserved superseded adapter work

The older worktree `/Users/oliver/Documents/Projects/RigWeave/desktop-web-ux-native-20260924` still contains its uncommitted v1.14 submodule/runtime pin experiment (including a local first branding change and refreshed lockfile). It was not reset, committed or extended. It is superseded by this dedicated product worktree.

## Exact unresolved work

- `SCQ-WEB-AUTH-001`: operator enrollment/pairing UI plus machine-token issuance, rotation and revocation workflow; the new read endpoints consume existing agent credentials but do not mint them.
- `SCQ-WEB-STATE-001`: authenticated station-state endpoint with authorization, TTL and persistence semantics.
- `SCQ-WEB-CONTACT-001`: adapt the canonical ingress to machine bearer auth and the strict full-QSO payload, retaining idempotency.
- `SCQ-DESKTOP-WIRING-001`: subscribe station publishing and completed-contact delivery to authoritative Nexus events and add the pairing UI. Digi logbook pull/status is wired; no synthetic radio state machine is permitted.
- `SCQ-WEB-REMOTE-001`: the existing Nexus Remote client is fail-closed at `remote-review.shackcq.invalid`; ShackCQ Web does not implement its `/api/remote/*` pairing protocol. It stays default-off.
- `SCQ-DIST-UPDATE-001`: ShackCQ-owned signing key and updater infrastructure.
- `SCQ-PACKAGE-MAC-001`: launch and safe packaged workflow evidence remain; the isolated app/DMG and checksum manifest now exist.
- `SCQ-PACKAGE-WIN-001`: Windows x64 artifact and evidence.
- `SCQ-NOTICE-001`: final candidate dependency/model/asset/proprietary-driver notice inventory.

No deployment, install, app launch, real audio, CAT, PTT, Tune, rotator movement, RF, external account, signing or notarization was performed.

## Final handoff snapshot

```text
RESULT: NOT_COMPLETE
UPSTREAM_BASELINE: Nexus v1.14.0 @ 12efe3d2970729465add40d62cecd76bbaf5b63f
NATIVE_HEAD: 12efe3d2970729465add40d62cecd76bbaf5b63f + dirty ShackCQ review patch set
WEB_HEAD: 98d56a2ac0a03a40028fd02309abcec191c439b7 + dirty contract patch; Nexus UI dist digest 58d77368820ea1aae3bce0ee35ce092be734b074956a98ffe5455f8e7b7cb8cf
INSTALLED_APP_IDENTITY: /Applications/ShackCQ Desktop.app; online.shackcq.desktop; 0.2.2; preserved
MATRIX_TOTAL: 30 applicable requirements + 1 pinned-upstream limitation
VERIFIED_SOFTWARE: 3 (SCQ-PACKAGE-022, SCQ-CONNECT-024, SCQ-MODES-029)
VERIFIED_PACKAGED_MAC: 0 (artifact statically verified; workflows not launched)
VERIFIED_PACKAGED_WINDOWS: 0
MISSING_OR_PARTIAL: NX-REMOTE-020, NX-UPDATE-021, SCQ-AUTH-025, SCQ-STATE-026, SCQ-CONTACT-027, SCQ-LOGBOOK-030
UPSTREAM_LIMITATIONS: NX-FOX-028 (FT8 Fox role not implemented in pinned upstream)
INTEGRATION_SUBSTITUTIONS: isolated ShackCQ package/data/vault identity; updater disabled; narrow website connector; details in matrix
MODE_RESULTS: Basic/Digi product split implemented; Basic defaults fail-closed and Digi retains the complete Nexus suite; packaged UI and hardware behavior not launched/exercised
MAC_ARTIFACT: src-tauri/target/release/bundle/macos/ShackCQ_Desktop_Nexus_Review_1.14.0-review.2_aarch64.dmg; SHA-256 66e2b2f9550d138c05b39f83e571a4f0e9da033d35d3c4e5932ea2d064b3217e; ad-hoc signed; not notarized
WINDOWS_ARTIFACT: none; Windows x64 packaging was not run on this macOS host
LINUX: DEFERRED; not certified by this task
CANONICAL_LOGGING: Digi automatically pulls the Web-selected canonical logbook after pairing and deduplicates through Nexus ADIF import; packaged/external fixture not run; completed-contact delivery remains PARTIAL at SCQ-CONTACT-027
HARDWARE_AUDIO_CAT: NOT EXERCISED / owner acceptance pending
RF_TX: NOT AUTHORIZED / NOT EXERCISED
PRODUCTION_DIGI_TX: unchanged, disabled; review build refuses TX arm
PRODUCTION_DEPLOYMENT: NOT PERFORMED
OWNER_REVIEW: OWNER_ACCEPTANCE_MACOS.md; safe review still requires explicit owner action
```
