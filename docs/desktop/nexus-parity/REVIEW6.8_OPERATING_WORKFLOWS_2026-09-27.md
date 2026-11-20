# Review 6.8 operating-workflow continuation

Date: 2026-09-27; final artifact completed 2026-09-28

Baseline: Nexus `v1.14.0` at `12efe3d2970729465add40d62cecd76bbaf5b63f`

## Preserved acceptance boundary

Review 6.7 native integration remains accepted at **6/6** for the Basic/Digi × logbook-authority matrix. Those rows were not synchronized again and the unchanged Basic suites were not rerun. No regression in those six accepted rows was observed during this continuation.

The three official WaveLogGate authority rows remain separate and explicitly **NOT RUN**:

| Official WaveLogGate row | Result | Exact decision outstanding |
| --- | --- | --- |
| Basic + Nexus Logbook authority | NOT RUN | An owner-authorized bootable guest with the actual official client and accepted licence is required. |
| Digi + Nexus Logbook authority | NOT RUN | An owner-authorized bootable guest with the actual official client and accepted licence is required. |
| Digi + Wavelog authority | NOT RUN | An owner-authorized bootable guest with the actual official client and accepted licence is required. |

No guest was installed or changed, no licence was accepted, and neither the official client nor the host gateway was touched.

## Packaged-interface defect and repair

The exact Review 6.7 application was exercised in its isolated acceptance root against the existing dummy rig and retained fixture input, with review TX disabled. The FT8 fixture decode and mode-selection paths were responsive, but retained receive/control commands for RTTY, PSK, APRS, JS8, SSTV and CW were absent from the acceptance IPC allowlist. This was a packaging/acceptance-boundary omission, not missing upstream decoder implementation. It caused visible packaged failures including the SSTV receiver-state error and inactive APRS/JS8 receive controls.

`src-tauri/src/acceptance.rs` now admits the bounded receive, decoder-state, mode-selection and fake-rig tuning commands required by those retained workflows. TX-capable commands remain denied, including the universal TX/tune controls and the RTTY, PSK, APRS, JS8, SSTV and CW send paths. The acceptance allowlist unit test passes with positive receive controls and negative TX controls.

The candidate identity was advanced locally to Review 6.8 because this is a meaningful workflow repair. It has not been signed, notarized, published, deployed, committed or pushed.

## Final local artifact

The unsigned macOS application bundle completed successfully from the Review 6.8 source and project UI:

- Bundle: `src-tauri/target/release/bundle/macos/ShackCQ Desktop Nexus Review 6.8.app`
- Bundle version: `1.14.0-review.6.8`
- Bundle identifier: `app.shackcq.desktop.nexusreview6`
- Architecture: arm64
- Bundle size: 75 MiB
- Main executable: 62,740,304 bytes
- Main executable SHA-256: `b083aed2e3b6f99331028b20193806b386daec05890c8af9cb2ab1dd12930246`
- Signing state: ad hoc/linker-signed only; no Team Identifier and no distribution signature

Per the continuation direction, the bundle was not launched and codec or packaged-workflow verification was not resumed. This is build and inventory evidence, not new runtime acceptance.

## Operating evidence obtained

- Review 6.7 packaged FT8: retained fixture decode active, Classic/Roster surfaces present, message rows visible, sequencing controls present, CAT connected to the isolated dummy rig, TX off.
- Review 6.7 packaged mode controls: FT4, Q65, MSK144, JT65 (RX-only), FST4, FST4W and WSPR selected with their retained mode-specific labels/frequencies; TX remained off. This is mode-control evidence, not per-mode on-air or automatic-QSO proof.
- Review 6.7 packaged Tempo: TempoFast and TempoDeep selection, conversation, roam and call controls present; TX remained off. This is cockpit/control evidence, not a completed HARQ session.
- Review 6.7 packaged Phone and CW: retained cockpits, status and receive/decode surfaces present; no physical audio, keying or PTT was exercised.
- Weak-signal codec regression: retained FT8, FT4, Q65, FST4/FST4W, MSK144, JT65, WSPR and TempoFast package tests passed, including encode/decode and rejection cases where provided by the pinned code.
- SSTV regression: decoder, resampling, VIS, encode, dropout, FSK-ID, manual receive, streaming and gallery/scope persistence groups exercised using generated or retained simulated audio only. Physical audio and PTT were not used.
- APRS regression: receive-only gesture, native stores, bounded display projection and safe dummy-rig tuning tests passed. No APRS-IS connection or RF path was exercised.
- JS8 regression: retained journal location, receive-only observation and native query-state tests passed. No message was transmitted.

A Nexus Logbook commit remains evidence for the accepted integration rows only; it is not treated as automatic-QSO, mode-specific decode or physical-operation proof.

## Candidate notices and corresponding-source inventory

- `COPYING` and `NOTICE` remain bundle resources; the complete pinned Nexus source plus the local ShackCQ patch set remains in this worktree.
- The Rust metadata graph contained 216 packages and no package without a declared licence expression.
- The installed frontend graph contained 281 package manifests and none without a declared licence field.
- DeepCW model metadata, published checksums, licence/readme material, OpenSSL notices, Nexus Remote notices and TLE resources are retained.
- The CQ-zone data licence already existed in source but was absent from the Review 6.7 bundle. It is now an explicit Review 6.8 bundle resource at `Resources/resources/data/cqzones.LICENSE.txt`.
- The exact Review 6.8 bundle contains the Hamlib notice/licence files but no Hamlib executable or dylib runtime. This remains an actual packaging omission for self-contained CAT/rotator operation. The pinned source-build staging script was policy-blocked in this environment and was not bypassed. A separately installed Nexus-derived application contains Hamlib 4.7.1 binaries, but read-only inspection of that installation is not substituted for reproducible staging into this candidate.

The exact Review 6.8 artifact inventory is now recorded, including the CQ-zone licence resource. `SCQ-LICENSE-023` remains open specifically for the missing Hamlib runtime/source correspondence.

## Remaining product implementation versus external gates

Actual product implementation still open:

- `NX-REMOTE-020` / `SCQ-WEB-REMOTE-001`: Nexus Remote server-protocol compatibility is absent and remains distinct from the already accepted ShackCQ website radio/rotator bridge.
- `NX-UPDATE-021` / `SCQ-DIST-UPDATE-001`: the unsafe upstream updater remains removed. A ShackCQ-owned signed update service, keys and release policy are still required; upstream endpoints and keys must not be restored.
- Self-contained macOS Hamlib runtime staging is incomplete as described above.
- Several retained operating rows still need exact Review 6.8 packaged workflow evidence before they can move beyond software/control evidence; the parity ledger names those gaps individually.

Distribution and owner-controlled gates, kept separate from implementation:

- ShackCQ signing identity, notarization/stapling and publication authority.
- Windows build and packaged acceptance environment.
- Owner radio, rotator, audio, PTT and on-air acceptance.
- Official WaveLogGate bootable guest and licence decision.
- Production deployment/write authorization and provider credentials.

No requirement has been waived or silently retired.
