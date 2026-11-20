# Review 5 execution prerequisites — 2026-09-26

**Action required to execute. OVERALL: NOT_COMPLETE.**

This addendum supersedes the earlier report's ambiguous packaged-journey statuses, not its historical evidence. Review 5 artifacts, production sources and original reports/manifests were left unchanged. No Review 6 was built. One agent performed this prerequisite audit; no implementation was delegated.

## Decision required

An owner/administrator must authorize the narrowly scoped disposable database lifecycle and isolated Review 5 launch through supported policy controls, or provide an already-authorized, verified disposable database. This session has approval policy `never`; it cannot submit a per-command escalation. The agent did not edit enforcement settings, retry either denied operation, switch execution tools to bypass them, or ask the owner to run them manually.

Authorization alone does not make the old launch command safe: source inspection below establishes that profile-only launch can inherit default settings and shared data. Complete fixture configuration/data/credential/child-process preflight before any authorized launch. Do not remove `--profile` or launch through the default application UI.

## Gate inventory

| Gate | Evidence | Required permission/resource | Authorized scope | Current result |
|---|---|---|---|---|
| Disposable PostgreSQL | Recorded pre-execution `initdb` allowlist denial; executable installed. Historical task cluster remains on disk, but no listener found on documented port 55441. | Supported authorization for exact initialization/start lifecycle, or verified already-approved instance with application role and synthetic tenants. | Task-owned data only, loopback TCP and private Unix socket, no production credentials. | POLICY_BLOCKED for new initialization; existing service NOT READY |
| Packaged macOS launch | Recorded pre-execution `open` allowlist denial. Extracted Review 5 identity and ad-hoc integrity verified. Isolation preflight incomplete. | Supported authorization for exact extracted app plus verified profile/environment/configuration; not the old profile-only command. | Private configuration AND data; exact fake endpoints; fixture credentials; no real I/O. | POLICY_BLOCKED; additional isolation prerequisite |
| Browser acceptance | Available controls: Chrome browser 3, profile Person1; Codex in-app browser 1. No test origin listener found on 4174. | Disposable browser session on actual fixture Web origin after services are healthy. | Synthetic account; do not reuse real account session. | UNAVAILABLE end-to-end; browser control available |
| Wavelog fixtures | Docker client cannot connect to `/Users/oliver/.docker/run/docker.sock`: socket absent. No verified disposable Built-in/External instances supplied. | Already-approved disposable real providers or authorized supported runtime. | Synthetic stations, credentials and contacts only. | UNAVAILABLE |
| Official WaveLogGate | No verified isolated official-client environment; installed client was not launched, addressed or changed. | Approved separate client configuration, listeners, credentials and destinations; unchanged official binary. | No traffic to installed gateway or its fixed listeners. | UNAVAILABLE |
| Windows | Installed Rust targets are Apple/Android only; `makensis` and `cargo-xwin` not found. No authorized Windows runner identified. | Authorized Windows x64 toolchain/packaging and interactive acceptance environment. | Source snapshot/build scripts only until authorized; no CI/push. | UNAVAILABLE |

## Exact recorded denials

Evidence below is the prior tool output retained in this conversation, not a freshly retried command. Current permitted configuration inspection is consistent with those denials. Tool metadata reports lean-ctx 3.10.3. PostgreSQL binary version and Codex application version were not established by these checks.

### PostgreSQL initialization

- Tool: `ctx_shell`.
- Executable: `/opt/homebrew/bin/initdb`.
- Arguments: `["-D", "/tmp/shackcq-review5-security-01a0db0b/postgres", "--auth=trust", "--username=shackcq_fixture"]`.
- Working directory: `/Users/oliver/Documents/Projects/RigWeave/desktop-web-ux-web-20260924`.
- Explicit extra environment: none. Inherited environment was not captured; no claim it was empty.
- Tool timeout: 120000 ms.
- Actual decisive denial text:

```text
[BLOCKED — DO NOT RETRY] 'initdb' is not in the shell allowlist. This is a permanent restriction, not a transient error.
Config in effect: /Users/oliver/.config/lean-ctx/config.toml
```

The tool returned `blocked=true` / `isError=true` before execution. PostgreSQL initialization did not begin. An existing empty-sized target directory is not evidence of a cluster or schema. Initialization itself would not start a listener; startup must be separately scoped and authorized.

### Packaged launch

- Tool: `ctx_shell`.
- Original executable spelling: `open`; permitted PATH inspection now resolves `/usr/bin/open`.
- Arguments: `["-na", "/Users/oliver/Documents/Projects/RigWeave/shackcq-desktop-nexus-20260925/src-tauri/target/release/bundle/macos/ShackCQ Desktop Nexus Review 5.app", "--args", "--profile=review5-isolated-smoke"]`.
- Working directory: `/Users/oliver/Documents/Projects/RigWeave/shackcq-desktop-nexus-20260925`.
- Explicit extra environment: `SHACKCQ_DIGI_TX_ENABLED=false`; inherited environment not captured.
- Tool timeout: 30000 ms.
- Actual decisive denial text:

```text
[BLOCKED — DO NOT RETRY] 'open' is not in the shell allowlist. This is a permanent restriction, not a transient error.
Config in effect: /Users/oliver/.config/lean-ctx/config.toml
```

The tool returned a pre-execution denial: launcher and app did not begin. This is not an app crash, Gatekeeper rejection, privacy prompt or macOS Automation denial. Do not approve OS prompts or strip metadata.

The next acceptance target must be the existing extracted app, not silently substituted for the denied source-bundle path:

`/Users/oliver/Documents/Projects/RigWeave/shackcq-desktop-nexus-20260925/src-tauri/target/release/bundle/review5-zip-verified/ShackCQ Desktop Nexus Review 5.app`

Retain arguments `--args --profile=review5-isolated-smoke` with `/usr/bin/open -na` only after supported authorization and complete isolation review. This is a proposed scoped target, not a launch-ready instruction or a request to execute outside policy.

### Enforcing layer and narrow workflow scope

The actual denials identify **lean-ctx shell allowlist** enforcement. Current `/Users/oliver/.config/lean-ctx/config.toml` has an additive executable list containing neither `initdb` nor `open`. Defaults are not enumerated in that file; absence from extras alone is not the proof. Owner/managed provenance of the configuration is UNKNOWN. There is no evidence these are Codex `.rules` decisions. The separate active Codex approval policy is `never`.

The owner's supported policy decision must cover only the disposable workflow, not unrestricted shell access: initialization under `/tmp/shackcq-review5-security-01a0db0b/postgres`; reviewed PostgreSQL start on `127.0.0.1:55441` with a private task-owned Unix socket; synthetic database/schema/application roles; actual API on `127.0.0.1:39878`; actual Web origin `http://127.0.0.1:4174`; extracted Review 5 app with verified isolation; exact fake-device endpoints once allocated and ownership verified. These ports are proposed historical fixture ports, not reserved or running services. Recheck conflicts before use. Stop only recorded task-owned process IDs and this exact cluster; preserve logs/data as evidence, do not run broad cleanup.

Existing `scripts/run-nexus-desktop-operator-fixture.sh` documents the real service lifecycle, but is not a permission bypass. Its broad temporary-tree cleanup must not be executed blindly. The earlier trust-auth initialization arguments are evidence, not blanket approval of trust-auth listeners. Verify application-role/RLS and tenant authorization separately from an administrative fixture connection.

The older `/tmp/shackcq-operator-fixture.my46jC/postgres` has cluster-shaped files and no `postmaster.pid` in its directory listing. `lsof` found no listeners on 55441, 39878 or 4174 (exit 1, no matches). Its HBA has local/loopback trust entries. Ownership of databases, credentials, schema readiness and task authorization for reuse are not established merely by its existence. Two `ctx_read` inspections of PG_VERSION/postmaster.opts were refused by that tool's project-root boundary (`/private/tmp` outside `/Users/oliver`); these reads were not rerouted. No cluster was started or queried. Do not call a stopped, unverified cluster an available approved service.

## Source-derived launch isolation: NOT READY

| Boundary | Verified current source fact | Remaining preflight |
|---|---|---|
| Configuration | `src-tauri/src/lib.rs:1408` uses XDG_CONFIG_HOME, otherwise HOME/.config; `:1454` parses the explicit profile; `:1481` names `shackcq-nexus-review-<profile>`. | Use a private task-owned config root. The earlier Application Support path check is not authoritative for this implementation. |
| Default inheritance | `lib.rs:22953` copies default settings into any named profile lacking settings.json, before `Settings::load`. | Precreate and validate synthetic settings in a private root with no default station settings. Profile name alone is unsafe. |
| Shared data/caches | `lib.rs:1724` uses NEXUS_DATA_DIR or the default profile's shared data location, independent of named profile. | Explicit private NEXUS_DATA_DIR; verify log, cache, gallery/recording paths and WebView state. Do not repurpose HOME. |
| Pairing/outbox | `shackcq_connector.rs:261` derives pairing, selection, outbox and receipts from config_dir. | Fresh fixture-only pairing record and actual native persistence/relaunch proof. |
| Credentials | Connector keychain service is fixed; alias is `agent-<agent_id>` (`:525`), not profile-namespaced. Provider keychains in `lib.rs:17075` use global service/user constants. | Generated noncolliding agent identity; ensure provider credentials never loaded/written from real aliases. Complete call-path review; a fresh bundle ID does not isolate the keychain. No credential values inspected here. |
| Single instance | `lib.rs:1551` has a named-profile advisory lock; startup `:22942` warns and continues on failure. | Enforce one task-owned instance; verify Quit/relaunch and child ownership. Lock presence alone is not exclusivity proof. |
| Audio/CAT | `lib.rs:23154` selects Disabled or explicit Fixture startup; Disabled still retains configured CAT. | Private Basic radio config must name exact task-owned fake endpoint. Digi must pin explicit file source/sink and verify no physical fallback. |
| Rotor/children | Retained controller can create/use native transports; no fake endpoint ownership established in this run. | Pin and verify exact fake rotor endpoint and child executables; no serial/network station fallback, no unverified loopback ports. |

No fixture audio environment, fake endpoint or safe launch environment was fabricated or declared ready. `SHACKCQ_DIGI_TX_ENABLED=false` remains mandatory but is not, by itself, a CAT/audio/provider isolation boundary.

## Frozen identity and permitted verification

Fresh checks: ZIP/DMG/extracted executable SHA-256 match the original manifest; extracted Info.plist is version `1.14.0-review.5`, identifier `app.shackcq.desktop.nexusreview5`; executable is arm64; `codesign --verify --deep --strict` passes. Signature inspection reports ad-hoc, no TeamIdentifier. Developer ID trust, notarization and Gatekeeper acceptance are not established. No launch was performed.

| Frozen artifact | SHA-256 |
|---|---|
| `src-tauri/target/release/bundle/macos/ShackCQ_Desktop_Nexus_Review_1.14.0-review.5_aarch64.zip` | `46c09ff22ab3732539dad7b0f5ec5fa5bed8db188d75bb38f44f01e6c233422a` |
| `src-tauri/target/release/bundle/macos/ShackCQ_Desktop_Nexus_Review_1.14.0-review.5_plain_aarch64.dmg` | `60f25a3205c740cf33c0c6cc011250e1a84101282ec6824719d3b5b3e66d5795` |
| Extracted app `Contents/MacOS/shackcq-desktop-nexus-review` | `de33833a5f5a842eb98d46b936d59c8ad2333fd49c65cfe20acd8deffc652607` |

Desktop current HEAD/branch: `12efe3d2970729465add40d62cecd76bbaf5b63f` / `codex/shackcq-nexus-product-20260925`, dirty. Web current HEAD/branch: `98d56a2ac0a03a40028fd02309abcec191c439b7` / `codex/desktop-web-ux-20260924`, dirty. **Manifest correction:** the frozen manifest's Web branch `codex/desktop-web-ux-web-20260924` is a typo; the original report and current Git agree on the shorter branch above. Frozen manifest left untouched.

Recorded build-input hashes (not independently recomputed as a full source closure in this audit): Desktop binary diff `a71c37e82560b5d08862de8e34a110531e98b666fa3b437c8a5c8cb8bb933bfb`; selected content manifest `1ea0e89384ff73116f2b1b4269e62bba6bc828ce8d10cecc223ee37ba7e71c92`. Neither upstream HEAD nor a selected-file hash alone establishes a complete dirty source/dependency/assets snapshot. Historical mounted-DMG verification was not rerun; container hash is fresh.

## Nine-row exact-candidate matrix

Focused tests below are the preserved Review 5 report's prior results, not newly rerun suites. No policy denial is classified as application FAIL.

| # | Mode / authority | Implementation | Focused verification | Exact packaged journey | Blocking layer / historical evidence |
|---|---|---|---|---|---|
| 1 | Basic / Local ADIF | PARTIAL acceptance closure; bridge/guards reported implemented | Prior focused PASSED; not rerun | BLOCKED | EXECUTION_POLICY + isolation preflight. Review 4 positive path historical only. |
| 2 | Basic / Built-in Wavelog | PARTIAL; provider-specific workflow unverified | NOT_RUN provider-specific | NOT_RUN | ENVIRONMENT plus launch policy |
| 3 | Basic / External Wavelog | PARTIAL; provider-specific workflow unverified | NOT_RUN provider-specific | NOT_RUN | ENVIRONMENT plus launch policy |
| 4 | Digi / Local ADIF | PARTIAL; journal/receipt/revision path reported implemented, legacy partition incomplete | Prior focused PASSED; not rerun | BLOCKED | EXECUTION_POLICY + isolation preflight; no packaged synthetic exchange/crash matrix |
| 5 | Digi / Built-in Wavelog | PARTIAL; actual provider workflow not established | NOT_RUN provider-specific | NOT_RUN | ENVIRONMENT plus launch policy |
| 6 | Digi / External Wavelog | PARTIAL; actual provider workflow not established | NOT_RUN provider-specific | NOT_RUN | ENVIRONMENT plus launch policy |
| 7 | Official WaveLogGate / Local ADIF | Compatibility implementation not independently closed | NOT_RUN official client | BLOCKED | ENVIRONMENT: isolated official client absent |
| 8 | Official WaveLogGate / Built-in Wavelog | Compatibility implementation not independently closed | NOT_RUN official client | BLOCKED | ENVIRONMENT: isolated official client and provider absent |
| 9 | Official WaveLogGate / External Wavelog | Compatibility implementation not independently closed | NOT_RUN official client | BLOCKED | ENVIRONMENT: isolated official client and provider absent |

Totals for exact candidate: **0 PASS, 0 FAIL, 5 BLOCKED, 4 NOT_RUN; native 0/6 PASS, official 0/3 PASS**. The old `2 PARTIAL` counted historical/component progress; it is not current packaged execution. No implementation percentage is inferred.

## Remaining software/evidence and requirement IDs

- `SCQ-WEB-AUTH-001`: exact packaged native credential persistence, Quit/relaunch, active revocation, denied reconnect and cross-tenant pairing remain blocked.
- `SCQ-WEB-STATE-001`: exact packaged independent readback, device refusal, stale presentation, generation/expiry rejection before dispatch and tenant telemetry/command denial remain blocked.
- `SCQ-WEB-CONTACT-001`: real packaged synthetic operating exchange into authoritative completion, all five durability/restart boundaries, lost ACK, fair delivery and strict receipt/tenant binding remain unexecuted.
- `SCQ-DESKTOP-WIRING-001`: exact fake-only startup/transition/rotor/child-process behavior and complete synthetic Digi lifecycle remain unaccepted; profile-only isolation is demonstrably insufficient.
- `SCQ-WEB-REMOTE-001`: **scope discrepancy remains open**. `NEXUS_PARITY_MATRIX.md:27` and `IMPLEMENTATION_AND_EVIDENCE.md:76` define this ID as default-off upstream Nexus Remote `/api/remote/*`, not the authenticated receive-safe agent bridge. The latter's tests cannot close the former. Retain default-off upstream Remote; its implement-or-explicitly-retire disposition is not established. No new remote-TX requirement added.
- `SCQ-PACKAGE-MAC-001`: integrity verified; isolated operator workflows blocked. Full source-to-artifact closure and runtime dependency completeness remain separate requirements.
- `SCQ-PACKAGE-WIN-001`: Windows build/package/launch absent; authorized environment needed. Existing source/scripts retained, no CI or new Windows artifact claimed.
- `SCQ-NOTICE-001`: bounded shipped-resource inventory below completed; exhaustive native/static/frontend dependency closure and corrective packaging still incomplete.
- `SCQ-LOGBOOK-RECONCILE-001`: revisioned implementation needs real provider/transition/conflict/interruption acceptance. **Legacy software gap:** `shackcq_connector.rs:1091` retains untagged, non-echo records in the active projection, then appends selected-authority records. Preservation is present; this is not an explicit isolated legacy partition. Non-contribution to another authority's worked-before/awards, explicit association UI and recovery/export journey are not established. Do not automatically delete or assign these records. No production data was read or migrated.
- `SCQ-MATRIX-001`: no exact-candidate row accepted; nine rows retained above.

`SCQ-DIST-UPDATE-001` remains historically CLOSED for upstream updater removal. No replacement ShackCQ update channel is claimed; no updater work reopened. Physical TS-480 proof remains historical and owner-only hardware acceptance distinct from software execution. Production Digi TX stays disabled.

## Bounded shipped-component inventory / distribution

Fresh extracted-resource listing and selected notice sections establish:

| Component/evidence | Frozen Review 5 package finding | Remaining issue |
|---|---|---|
| Nexus attribution | Resources/NOTICE and Resources/COPYING present | Full corresponding dirty source closure not proven by upstream HEAD |
| Hamlib | AUTHORS, README, LICENSE, COPYING and COPYING.LIB texts present; no rigctl/rigctld/rotctl/rotctld binary in extracted file inventory | NOTICE:366 says runtime bundled on every platform; that claim does not match this app. Do not count host-installed tools as shipped runtime. |
| DeepCW | model.onnx, model metadata, README and AGPL text present | No new runtime/model acceptance or full native graph verification |
| Remote frontend | THIRD-PARTY, Radix license/package manifest, react-remove-scroll-bar license/package manifest present | Does not prove complete Desktop embedded frontend dependency closure |
| OpenSSL | License text present | Presence of a notice is not proof the library is linked |
| CQ zones | NOTICE:453 names data origin and adjacent cqzones.LICENSE.txt | No standalone CQ-zone license found in extracted resource inventory; complete embedded asset/notice correspondence unresolved |
| TLE resources | tles.json present | Broader provenance assertions remain those of the frozen NOTICE |

This is a file/provenance inventory, not a legal compliance certification. No frozen notice or binary was modified. Corrective packaging must be distinctly versioned and tested after execution prerequisites permit acceptance, not manufactured as another untestable checkpoint.

## Safety and handoff

Only permitted inspections and this documentation addendum were performed. No actual integration test executed, no application failure observed, no database initialized/started, no fixture traffic sent, no installed application changed/stopped, no keychain secrets accessed, no real radio/audio/rotor/PTT/Tune/CW/FSK/DAX/RF activity, no production writes, no OS consent approval or security metadata removal, no policy edits, commit/push/deployment/publication/external CI. No cleanup deletion performed.

Owner/admin action: authorize the scoped execution workflow through supported controls or identify a verified already-approved disposable environment. Before executing it, finish the explicit isolation configuration and review outstanding credential/cache/child-process boundaries. Until then the required acceptance remains blocked and the programme remains NOT_COMPLETE.
