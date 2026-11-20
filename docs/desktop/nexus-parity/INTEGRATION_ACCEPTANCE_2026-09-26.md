# ShackCQ Desktop and WaveLogGate operational acceptance — Review 5 — 2026-09-26

## Result

`NOT_COMPLETE`.

Nine-row result: **0 complete, 2 partial, 3 blocked, 4 not run**. Native macOS rows passed: **0/6**. Official WaveLogGate rows passed: **0/3**.

Review 5 materially closes implementation gaps in command freshness, fixture-audio isolation, the contact commit/outbox crash seam and authority-partitioned reconciliation. The exact Review 5 app, ZIP and plain DMG were built and verified. It does not claim the required packaged browser/operator journeys: the disposable PostgreSQL service could not be initialized under the enforced command allowlist, and the isolated app launcher could not pass the required fresh profile without using a blocked command. The default profile was deliberately not used because it could attach to configured physical hardware.

## Exact source identities

- Desktop worktree: `/Users/oliver/Documents/Projects/RigWeave/shackcq-desktop-nexus-20260925`; branch `codex/shackcq-nexus-product-20260925`; HEAD `12efe3d2970729465add40d62cecd76bbaf5b63f`; dirty checkpoint.
- Desktop binary diff SHA-256: `a71c37e82560b5d08862de8e34a110531e98b666fa3b437c8a5c8cb8bb933bfb`.
- Selected tracked/untracked Desktop content-manifest SHA-256: `1ea0e89384ff73116f2b1b4269e62bba6bc828ce8d10cecc223ee37ba7e71c92`.
- Web worktree: `/Users/oliver/Documents/Projects/RigWeave/desktop-web-ux-web-20260924`; branch `codex/desktop-web-ux-20260924`; HEAD `98d56a2ac0a03a40028fd02309abcec191c439b7`; dirty checkpoint.
- Official WaveLogGate reference: unmodified v2.1.1 identity `9344a4852ad461b60a50fbb384e2afdf17f6e414`. The installed client and listeners were not used or modified.

## Review 5 artifacts

- App: `src-tauri/target/release/bundle/macos/ShackCQ Desktop Nexus Review 5.app`.
- ZIP: `src-tauri/target/release/bundle/macos/ShackCQ_Desktop_Nexus_Review_1.14.0-review.5_aarch64.zip`; 39,605,654 bytes; SHA-256 `46c09ff22ab3732539dad7b0f5ec5fa5bed8db188d75bb38f44f01e6c233422a`.
- Plain DMG: `src-tauri/target/release/bundle/macos/ShackCQ_Desktop_Nexus_Review_1.14.0-review.5_plain_aarch64.dmg`; 43,743,848 bytes; SHA-256 `60f25a3205c740cf33c0c6cc011250e1a84101282ec6824719d3b5b3e66d5795`.
- Version/bundle: `1.14.0-review.5`, `app.shackcq.desktop.nexusreview5`, arm64.
- Signed executable SHA-256: `de33833a5f5a842eb98d46b936d59c8ad2333fd49c65cfe20acd8deffc652607`.
- App, extracted ZIP and mounted DMG executable identities matched. Strict/deep signature verification and DMG checksum verification passed. The image contained only the Review 5 app and detached cleanly.
- Signature is ad-hoc integrity only. TeamIdentifier is absent; Developer ID, notarization and Gatekeeper acceptance are not claimed.
- Executable dynamic linkage contains Apple system frameworks/libraries only.

Detailed artifact evidence: `docs/desktop/nexus-parity/MACOS_REVIEW5_ARTIFACT_MANIFEST_2026-09-26.json`.

## Implemented behavior

### Basic command and credential boundary

- Radio commands now carry a required expiration time. Web rejects invalid, expired and over-long commands before agent lookup/send.
- Per-agent command generations remain monotonic across reconnects. Desktop validates protocol, identity, generation, expiration and parameter size before Nexus mutation.
- Existing Review 4 positive packaged Basic + Local ADIF evidence remains historical evidence for the unchanged controller path. It is not presented as verification of the Review 5 executable.

### Hardware-free Digi runtime

- Basic/controller-only startup does not construct or query CPAL and keeps CAT/rotator/pairing/logging independent of audio.
- Explicit Fixture mode uses a bounded f32le source and file sink. It cannot fall back to System default audio.
- Fixture policy survives settings rebuilds: no CAT/serial/network/OmniRig transport, native CI-V, Flex DAX, Yaesu USB spectrum, background monitor/handoff, serial CW/WinKeyer/FSK, ATU or voice-memory CAT output. CW renders only to the file sink.
- WSJT-X and PSK Reporter emitters are disabled outside Device mode. `SHACKCQ_DIGI_TX_ENABLED=false` remains required.

### Contact durability and reconciliation

- Nexus emits prepare/commit lifecycle observations around stable contact identity. Commitment is emitted only after successful append.
- Delivery revalidates and synchronizes the actual local log file rather than trusting in-memory presence. Temporary atomic-write files are ignored, flush is serialized, and the delivery cap is applied after destination eligibility.
- Canonical receipts require matching event, account/station, authority/revision/integration and a valid durable receipt before outbox cleanup. A stored receipt permits restart cleanup without re-send.
- Complete bounded snapshots carry authority revision, partition identity, stable QSO IDs, row revisions and tombstones. Web snapshot reads are repeatable-read coherent.
- Desktop persists partition caches, rejects obsolete responses, rechecks authority after download and atomically replaces identified authoritative rows. Edits, QSL revocation and deletions are represented; imports bypass local completion observers/provider upload queues to prevent echo writers.
- Legacy additive imports have no stable remote provenance and are therefore preserved. Automatically deleting or merging those untagged rows would risk deleting operator contacts. Fresh Review 5 identified rows use the revisioned path.

## Focused verification

- ShackCQ agent native tests: 7 passed.
- Connector/outbox/reconciliation native tests: 9 passed.
- Station projection durability/no-upload regression: 1 passed.
- Controller-only factory regression: 1 passed.
- Fixture transport sanitizer regression: 1 passed.
- Desktop product-mode UI tests: 3 passed.
- Web Nexus Desktop contract tests: 6 passed.
- Web radio-agent contract/hosted focused tests passed; hosted TypeScript check passed.
- Desktop and Web `git diff --check` passed; Rust formatting checks passed.
- Fresh Desktop UI `tsc -b && vite build` passed.
- Final optimized Rust build passed in 15m18s and the fresh UI was embedded before bundling.

These are supporting checks. They do not replace packaged operator-workflow acceptance.

## Nine-row integration matrix

| # | Bridge/mode | Authority | Result | Evidence and exact remaining gap |
|---:|---|---|---|---|
| 1 | Desktop Basic | Local ADIF | `PARTIAL` | Review 4 exact packaged positive pairing, persistence, zero-audio CAT, telemetry, radio readback and native rotor target/readback/Stop remain valid for Review 4. Review 5 stale/expiry/refusal and generation enforcement is tested at contract/native boundaries, but active revocation, refused device, two-tenant and positive smoke were not rerun through the exact Review 5 package/browser/service. |
| 2 | Desktop Basic | Built-in Wavelog | `NOT_RUN` | No disposable real managed Wavelog instance/database was available. |
| 3 | Desktop Basic | External Wavelog | `NOT_RUN` | No disposable real external Wavelog instance/database was available. |
| 4 | Desktop Digi | Local ADIF | `PARTIAL` | Hardware-isolated runtime, durable journal/outbox/receipt and revisioned reconciliation are implemented and focused tests pass. No packaged decoder/sequencer exchange reached Nexus's authoritative contact lifecycle through the exact app, and the four crash points were not exercised against the canonical disposable service/database. |
| 5 | Desktop Digi | Built-in Wavelog | `NOT_RUN` | No disposable real managed provider; provider delivery/retry/reconciliation not executed. |
| 6 | Desktop Digi | External Wavelog | `NOT_RUN` | No disposable real external provider; provider refusal/retry/echo path not executed. |
| 7 | Official WaveLogGate | Local ADIF | `BLOCKED` | No verified task-isolated official-client runtime/config/listener/destination environment. Installed client untouched; no mock substituted. |
| 8 | Official WaveLogGate | Built-in Wavelog | `BLOCKED` | Same isolated official-client blocker plus no disposable managed provider. |
| 9 | Official WaveLogGate | External Wavelog | `BLOCKED` | Same isolated official-client blocker plus no disposable external provider. |

## Precise execution blockers

- Disposable canonical service: PostgreSQL `initdb` was refused by the enforced lean-ctx allowlist; Docker is not running. The allowlist/security configuration was not changed.
- Exact isolated packaged launch: the required `open -na ... --args --profile=review5-isolated-smoke` command was refused by the same allowlist. UI launch without the profile argument was rejected as unsafe because it could use Oliver's configured hardware profile. No Gatekeeper or OS prompt was approved.
- Built-in/External Wavelog: no disposable real instances/databases were available.
- Official WaveLogGate: no isolated official-binary environment was available. The installed running gateway was not touched.
- Windows: installed Rust targets are Apple/Android only; no Windows x64 target, `makensis`, `cargo-xwin` or authorized runner is available. No external CI was triggered.

## Official WaveLogGate inherited limitations

- Published local HTTP/WebSocket listeners do not implement local authentication.
- The outbound client disables TLS certificate verification. ShackCQ does not copy this behavior.
- Published protocol does not establish rotor readback or rotor Stop.
- `localhost` is local to the station computer; another computer's browser cannot use its own localhost to control the station.
- Upstream configuration stores the Wavelog credential in JSON with mode `0644`.
- Completed-QSO input is the Secondary UDP logger path, not a full Digi modem/control contract.

## Requirement ledger

- `SCQ-WEB-AUTH-001` — **PARTIAL**: pairing/persistence/reconnect retained; active revocation/re-pair/two-tenant packaged journey not run on Review 5.
- `SCQ-WEB-STATE-001` — **PARTIAL**: expiry/generation validation implemented and focused tests pass; packaged refusal/stale/tenant journey not run.
- `SCQ-WEB-CONTACT-001` — **PARTIAL**: durable commit/outbox/receipt implementation and tests pass; packaged authoritative completion/crash matrix not run.
- `SCQ-DESKTOP-WIRING-001` — **PARTIAL**: Basic audio independence and isolated Fixture policy pass focused verification; no packaged synthetic completed exchange.
- `SCQ-WEB-REMOTE-001` — **PARTIAL**: authenticated receive-safe route retained and freshness guards added; Review 5 integrated negative journey not run.
- `SCQ-PACKAGE-MAC-001` — **PARTIAL**: exact Review 5 app/ZIP/plain DMG verified; required operator journeys were not run from the package.
- `SCQ-PACKAGE-WIN-001` — **OPEN**: required Windows execution environment/toolchain unavailable.
- `SCQ-DIST-UPDATE-001` — **CLOSED**: retained upstream updater removal; no replacement update channel advertised.
- `SCQ-NOTICE-001` — **OPEN**: Hamlib runtime is claimed but absent from the bundle; desktop transitive-license generation and CQ-zone license packaging remain unresolved. Final static/native and frontend graph-to-artifact correspondence remains incomplete.
- `SCQ-LOGBOOK-RECONCILE-001` — **PARTIAL**: revisioned identified-row implementation and focused tests pass; real provider, conflict/retry and packaged authority-transition journeys remain.
- `SCQ-MATRIX-001` — **OPEN**: no fully accepted row.

Remaining IDs: `SCQ-WEB-AUTH-001`, `SCQ-WEB-STATE-001`, `SCQ-WEB-CONTACT-001`, `SCQ-DESKTOP-WIRING-001`, `SCQ-WEB-REMOTE-001`, `SCQ-PACKAGE-MAC-001`, `SCQ-PACKAGE-WIN-001`, `SCQ-NOTICE-001`, `SCQ-LOGBOOK-RECONCILE-001`, `SCQ-MATRIX-001`.

## Safety record

- `SHACKCQ_DIGI_TX_ENABLED=false` throughout.
- The reconnected TS-480 was not opened, queried, retuned or keyed. Historical physical proof was not reused as Review 5 binary proof.
- No real serial/radio/audio/PTT/Tune/CW/FSK/DAX/network-radio output, RF or rotor movement.
- No production contacts/provider writes; no installed application changed or stopped.
- No fixture traffic reached installed WaveLogGate listeners.
- No OS prompt approval, Gatekeeper bypass, deployment, publication, commit, push or external CI.
