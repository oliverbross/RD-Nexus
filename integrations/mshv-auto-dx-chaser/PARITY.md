# DX Chaser parity ledger

Reviewed upstream: `kd9taw/mshv-auto-dx-chaser@d960ae22de78940c6be9d95bd4817d233d02ee39`.
Reviewed Nexus baseline: `kd9taw/Nexus@1150c8dd9db714fecd12182ae26bc8d72b92f444` (Landing 62).

This ledger is the source-behavior acceptance record for the behavioral port. `VERIFIED`
means the upstream behavior is implemented in the DX Chaser engine or adapter and has
executable evidence. `VERIFIED SHARED AUTHORITY` means Nexus deliberately supplies the
same operator-visible behavior through an existing single-owner subsystem. It does not
mean that the upstream Qt object, private database, network client, or transmit path was
copied into Nexus.

Source parity and release/runtime evidence are independent gates. The source-parity gate
passes at this reviewed revision. The existence and operation of signed or packaged
macOS, Windows, and Linux artifacts, an on-air radio, and a real Twilio account remain
separate external evidence and are not asserted by this ledger.

## Source and behavior matrix

| Upstream source / behavior | Nexus implementation or shared authority | Executable evidence | Status |
|---|---|---|---|
| `HvAutoDxer/autodxercore.cpp`: queue ingestion, need-only target selection, engage lock, missed-cycle handling, scarce persistence, ATNO cap, cooldowns, queue removal/reinsert, and same-band non-standard priority | `dx_chaser::Chaser::{ingest_decode,candidates,cycle,observe_queue}`; the Tauri adapter maps the matching attempt to Nexus's per-attempt sequencer and observes every fresh FT log row. Per-attempt completion/queue flush supplies upstream `StopAuto` semantics without revoking the higher-level operator Enable-TX permission. | `candidates_require_a_need_and_use_exclusive_need_priority`; `engage_lock_survives_missed_cycles`; `ordinary_target_abandons_on_second_miss_and_can_reselect_cached_decode`; `continuously_heard_atno_stops_on_total_cycle_twenty`; `engaged_atno_still_obeys_total_cycle_hard_cap`; `worked_cooldown_uses_base_calls_and_strict_boundary`; `queue_presence_replays_remove_and_reinsert_transitions`; `same_band_nonstandard_alert_precedes_local_candidate`; `queue_presence_binds_only_the_active_attempt_base_call`; `unrelated_logged_qso_updates_counts_without_clearing_active_target` | VERIFIED SHARED AUTHORITY |
| `autodxercore.cpp`: cluster/RBN/PSK Reporter nomination, cross-band verification, timeout/return, and local-call gate | Shared Nexus cluster, RBN-labelled cluster, and PSK Reporter paths nominate or QSY only; only a fresh local decode can become a call action. Unassisted mode suppresses external evidence. | `never_calls_external_only`; `corroborated_external_need_changes_band_for_local_verification_only`; `external_qsy_decision_expires_after_120_seconds_even_with_long_retention`; `cross_band_nomination_locks_to_call_and_preserves_rare_frequency`; `same_band_external_nonstandard_qsy_waits_through_sixty_then_returns`; `chaser_external_sources_accept_only_ft8_and_ft4` | VERIFIED SHARED AUTHORITY |
| `autodxercore.cpp`: QSO completion, statistics, reset, and return from a temporary dial | Matching fresh canonical Nexus logbook rows complete an attempt; unrelated/replayed rows cannot. Session counters reset independently from versioned all-time counters. | `only_matching_fresh_canonical_qso_completes_attempt`; `matching_logged_qso_cannot_recall_cached_target_without_new_decode`; `unrelated_logged_qso_updates_counts_without_clearing_active_target`; `session_reset_preserves_persisted_all_time_statistics`; `version_two_history_migrates_legacy_statistics_to_all_time`; `logged_qso_signature_suppresses_replayed_log_ticks` | VERIFIED SHARED AUTHORITY |
| `HvAutoDxer/autodxerpanel.cpp`: start/stop, candidates, cache, log, statistics, settings, and operator status | Native `DxChaserView` provides the controller, target/status, Candidates, DXCC, Bands, Cache, Log, Statistics, and Settings surfaces. Stop is sticky and disarms without acquiring transmit authority. | `automation_arm_is_distinct_and_stop_is_sticky`; `action_failure_disarms_clears_target_and_surfaces_error`; `DxChaserView.dxcc.test.tsx`: `shows authoritative feed health, source controls, SMS interval, and split statistics` | VERIFIED |
| `HvAutoDxer/bandmanager.cpp`: standard FT8 dial table, enabled bands, time exclusions, and propagation/activity choice | The portable engine owns the reviewed nine-band default set and standard dials; ordinary ranking uses current activity, needed spots, time schedule, persisted decode feedback, dwell, and hysteresis. Actual CAT changes go through Nexus Engine. | `default_bands_exactly_match_upstream_auto_band_set`; `time_schedule_excludes_low_bands_only_during_local_day`; `ordinary_band_switch_obeys_dwell_boundary_and_strict_hysteresis`; `ordinary_ranking_uses_persisted_penalty_history_and_bailout_without_bypassing_gates` | VERIFIED SHARED AUTHORITY |
| `HvAutoDxer/decodecache.cpp`: local/external lifetimes, one refreshed record per call/band, repeat count, expiry, and active-target behavior | Bounded `Decode`/`Spot` caches in the portable engine retain source identity and upstream boundary semantics. | `local_and_external_ttl_boundaries_are_distinct`; `external_spotter_confidence_and_cache_are_bounded`; `active_selection_and_cache_expiry_match_upstream_boundaries`; `decode_cache_refreshes_one_entry_and_preserves_repeat_count` | VERIFIED |
| `HvAutoDxer/dxcctracker.cpp` and `HvDxccTracker/hvdxcctracker.cpp`: ADIF/log history, worked DXCC/band/grid facts, CRUD, import/export, and tracker statistics | Nexus's canonical all-mode SQLite logbook is the single tracker database and import/export authority. DX Chaser consumes its worked/need facts and does not create a divergent private tracker. | `only_matching_fresh_canonical_qso_completes_attempt`; `matrix_keeps_band_order_and_only_overlays_unworked_spots`; existing Nexus logbook test suites and the 17-test focused Tauri adapter run | VERIFIED SHARED AUTHORITY |
| `HvAutoDxer/dxclusterclient.cpp`, `HvAutoBand/hvrbntelnet.cpp`, and connection portions of `spotaggregator.cpp` / `hvspotcollector.cpp`: endpoints, parsing, reconnect, source enablement, errors, and health | Central Nexus source endpoints, parsers, reconnect loops, and `get_feed_health` are the single network authority. DX Chaser exposes per-source use toggles and live health, but intentionally has no per-feature RBN host/port control. | `source_use_settings_default_on_survive_serde_and_gate_only_their_source`; `chaser_external_sources_accept_only_ft8_and_ft4`; `adapter_ingest_respects_us_spotters_and_rare_dx_exemption`; `DxChaserView.dxcc.test.tsx`: `shows authoritative feed health, source controls, SMS interval, and split statistics` | VERIFIED SHARED AUTHORITY |
| `HvAutoDxer/localspotbridge.cpp`: distinguish local decoder output from network evidence | The adapter feeds `Decode` only from Nexus's local recent-decoder authority; network evidence enters as `Spot`. | `never_calls_external_only`; `corroborated_external_need_changes_band_for_local_verification_only` | VERIFIED SHARED AUTHORITY |
| `HvAutoDxer/spotaggregator.cpp`: aggregation, source confidence, US spotters, rare exemptions, need facts, and cleanup | Shared Nexus RBN/cluster/PSKR facts are normalized into bounded `Spot` evidence; canonical identity and grid authorities provide need and location facts. | `external_spotter_confidence_and_cache_are_bounded`; `us_spotter_filter_matches_upstream_prefixes_and_rare_exemption`; `us_grid_validation_matches_upstream_boundaries_and_territories`; `adapter_ingest_respects_us_spotters_and_rare_dx_exemption`; `matrix_includes_received_and_regional_psk_reporter_paths_only` | VERIFIED SHARED AUTHORITY |
| `HvAutoDxer/targetscorer.cpp`: exact need priority, 10k/1k/500/200 weights, SNR/repeat/spotter terms, and Jan-2025 Club Log ranking table | Portable scorer uses mutually exclusive need tiers, reviewed rarity data, normalized identity, and upstream SNR boundaries. Presentation-only reason strings are not behavioral input. | `upstream_score_vector`; `upstream_min_snr_boundary_accepts_minus_20_and_rejects_minus_21`; `candidates_require_a_need_and_use_exclusive_need_priority`; `rarity_bands_match_upstream` | VERIFIED |
| `HvAutoDxer/twiliosmsnotifier.cpp`: disabled/Every-TX/QSO-only policy, exact fact content, 60-second default rate limit, bounded retry, and test send | Portable `SmsOutbox` schedules messages; adapter uses Twilio REST with OS-keyring-only credentials. Explicit 429 responses are retryable; ambiguous transport failures are discarded to avoid duplicate delivery. Client and transport failures cross the adapter boundary only as fixed redacted messages. | `sms_outbox_rate_limits_retries_and_drops_after_bound`; `sms_interval_is_migrated_and_clamped_to_upstream_range`; `sms_messages_match_upstream_facts_and_utc_shape`; `ambiguous_sms_delivery_is_discardable_without_duplicate_retry`; `twilio_retries_only_an_explicit_rate_limit_response`; `unconfigured_twilio_drops_tx_and_qso_actions_without_queue_or_retry`; `twilio_transport_errors_never_expose_the_account_url_or_source_chain`; `twilio_phone_numbers_are_trimmed_and_normalized_to_e164` | VERIFIED SHARED AUTHORITY |
| `HvAutoBand/hvbandscorer.cpp`: normal/hunter formulas, needs, signal, time, recency, feedback penalty, and persisted history | `hv_band_score`, `BandPerformance`, and ordinary band ranking implement the upstream vectors and consume Nexus spot/logbook/location facts. | `band_scorer_matches_upstream_normal_hunter_penalty_and_history_vectors`; `decode_feedback_matches_upstream_ema_penalty_and_bailout`; `cycle_feeds_completed_decode_periods_even_while_disarmed`; `ordinary_ranking_uses_persisted_penalty_history_and_bailout_without_bypassing_gates` | VERIFIED SHARED AUTHORITY |
| `HvAutoBand/hvbandswitcher.cpp`: enablement, dwell, hysteresis, schedule, feedback, bailout, and deferred hunter pounces | Portable band policy produces guarded actions; actual band movement is performed by Nexus Engine. Its DX Chaser QSY verb validates the bound halt generation, flushes queued transmit work, preserves only the already-authorized Enable-TX latch, and returns the new generation for safe arm rebinding. | `ordinary_band_switch_obeys_dwell_boundary_and_strict_hysteresis`; `deferred_hunter_pounce_is_fifo_deduplicated_and_locally_safe`; `deferred_pounce_expires_strictly_after_upstream_boundary`; `band_change_records_feedback_and_clears_decode_target_history`; `time_schedule_excludes_low_bands_only_during_local_day`; `dx_chaser_qsy_refuses_an_unarmed_or_stale_generation_without_mutation`; `dx_chaser_same_band_qsy_flushes_emissions_and_rebinds_the_arm`; `dx_chaser_cross_band_qsy_keeps_routing_and_returns_the_new_generation` | VERIFIED SHARED AUTHORITY |
| `HvAutoBand/hvdecodetracker.cpp`: per-period decode metrics, expected-spot ratio, EMA, penalties, severe bailout, and durable history | `BandPerformance` is fed at completed slot periods, serialized in versioned DX Chaser history, and participates in ordinary ranking and bailout. | `decode_feedback_matches_upstream_ema_penalty_and_bailout`; `cycle_feeds_completed_decode_periods_even_while_disarmed`; `ordinary_ranking_uses_persisted_penalty_history_and_bailout_without_bypassing_gates`; `band_change_records_feedback_and_clears_decode_target_history` | VERIFIED |
| `HvAutoBand/hvpskreporter.cpp`: received/regional FT8/FT4 intelligence and error/health visibility | Nexus PSK Reporter is the sole endpoint/query authority. DX Chaser consumes own received and regional paths as nomination evidence and displays central feed health. | `matrix_includes_received_and_regional_psk_reporter_paths_only`; `chaser_external_sources_accept_only_ft8_and_ft4`; `DxChaserView.dxcc.test.tsx`: `shows authoritative feed health, source controls, SMS interval, and split statistics` | VERIFIED SHARED AUTHORITY |
| `HvAutoBand/hvslotfinder.cpp`: two-period readiness, rounded occupancy, preferred/callable ranges, least-congested fallback, and multistream clearance | Portable slot analysis selects an offset; Nexus's guarded sequencer remains the transmit-frequency authority. | `slot_finder_avoids_occupied`; `slot_finder_requires_two_periods_and_uses_rounded_neighbour_clearance`; `multistream_range_matches_upstream_offsets_and_clamps`; `slot_selection_clamps_then_avoids_multistream_occupancy` | VERIFIED SHARED AUTHORITY |
| `HvAutoDxSettings/hvautodxsettings.cpp`: defaults, all behavioral groups, save/load, migration, and corruption handling | Serde-compatible `Settings`, versioned `PersistentState`, and the native Settings tab cover master control, sources, bands, filters/scoring, chase/safety, SMS, and display options. Adapter persistence uses same-directory atomic replacement and safe fallback; the entire durable-save/runtime-apply transaction is serialized, and credentials stay in the OS keyring. | `settings_backward_compatibility_uses_upstream_band_defaults`; `settings_apply_upstream_clamps_and_safe_ranges`; `deserialized_zero_and_oversize_settings_are_safely_normalized`; `settings_loader_handles_missing_old_corrupt_and_truncated_files`; `atomic_json_failure_preserves_last_known_good_file`; `concurrent_atomic_writers_leave_one_complete_json_and_no_temp_debris`; `configuration_transaction_keeps_last_durable_and_runtime_settings_identical`; `configuration_save_failure_leaves_durable_and_runtime_settings_unchanged`; `DxChaserView.dxcc.test.tsx`: `exposes scarce mode as an explicit saved setting` | VERIFIED SHARED AUTHORITY |
| `HvDxccChaser/dxccbandindicator.cpp` and `hvdxccchaserwindow.cpp`: nine-band worked/spot matrix, filters, search, row details, and refresh | Read-only native DXCC tab combines canonical worked facts with current Nexus cluster/RBN/PSKR evidence. It exposes no direct call/QSY control. | `matrix_keeps_band_order_and_only_overlays_unworked_spots`; `matrix_includes_received_and_regional_psk_reporter_paths_only`; `DxChaserView.dxcc.test.tsx`: `renders the nine-band matrix, filters entities, and exposes spots without radio actions`; `refreshes the visible DXCC matrix every 30 seconds and stops after leaving` | VERIFIED SHARED AUTHORITY |

## Safety and authority matrix

| Control | Nexus rule | Executable evidence | Status |
|---|---|---|---|
| Launch and automation arm | Disabled by default; DX Chaser's arm is independent from Nexus Enable TX and cannot raise it. | `automation_arm_is_distinct_and_stop_is_sticky`; `tx_latch_is_mandatory` | VERIFIED SHARED AUTHORITY |
| Identity, grid, licence, watchdog, decoder, CAT, PTT, and sequencing | Guarded `tempo-app::Engine` remains the only radio/transmit authority. The portable crate owns no CAT, PTT, audio, decoder, licence, identity, or watchdog handle. | `dx_chaser_call_never_raises_the_tx_latch`; focused Tauri suite and `cargo check --manifest-path src-tauri/Cargo.toml --features radio,custom-protocol` | VERIFIED SHARED AUTHORITY |
| Local verification | Network evidence may nominate or trigger a verification QSY, never a call. | `never_calls_external_only`; `corroborated_external_need_changes_band_for_local_verification_only` | VERIFIED |
| Stop and failure | Stop is sticky; adapter failures halt transmit, clear the target, restore guarded radio context, and disarm. Arming observes TX authority and its halt generation atomically, and every Call is bound to that generation, so Stop TX followed by a later re-enable cannot revive a stale action. Queue presence is matched to the active attempt, which is the Nexus equivalent of upstream `StopAuto`. | `automation_arm_is_distinct_and_stop_is_sticky`; `stop_during_arm_observation_cannot_bind_authority_for_a_later_reenable`; `dx_chaser_call_rejects_a_retired_arm_after_stop_and_reenable_without_mutation`; `action_failure_disarms_clears_target_and_surfaces_error`; `queue_presence_replays_remove_and_reinsert_transitions`; `queue_presence_binds_only_the_active_attempt_base_call` | VERIFIED SHARED AUTHORITY |
| Credentials | Twilio secrets are stored in the OS keyring; legacy values migrate without adding secrets to DX Chaser settings/history. | focused Tauri credential/configuration tests; adapter architecture inspection | VERIFIED SHARED AUTHORITY |

## Audited non-behavior and dead-code exclusions

The following reviewed upstream surfaces are intentionally not parity requirements because
they have no distinct consumed operator behavior at the pinned revision:

- `HvAutoDxSettings/hvautodxstatuspanel.{h,cpp}` is not constructed or referenced outside
  its own two files at `d960ae22`; its status concepts are nevertheless represented by the
  live DX Chaser header, target card, source health, log, and statistics panels.
- Qt widget geometry, palette/style-sheet constants, signal/slot plumbing, debug-only text,
  and scorer/slot-finder explanation strings are implementation and presentation details,
  not controller inputs or externally persisted protocol. Nexus uses its native design
  system and equivalent state/error fields.
- Duplicate private CAT/PTT/decoder, tracker-database/import, identity/grid, key storage,
  and source-client implementations are unconsumed by design. Their observable behaviors
  are covered above by explicit `VERIFIED SHARED AUTHORITY` rows; instantiating them would
  create conflicting owners rather than additional parity.

This exclusion is narrow: upstream behavior that is instantiated or consumed is represented
in the source matrix even when its implementation is replaced by a shared Nexus authority.

## Verification inventory

| Layer | Command / evidence | Result |
|---|---|---|
| Portable Rust decision engine | `cargo test -p dx-chaser` | 53 passed |
| Generation-bound Engine call guard | `cargo test -p tempo-app dx_chaser -- --nocapture` | 6 passed; 1,654 filtered |
| Focused Tauri adapter and guard coverage | `cargo test --manifest-path src-tauri/Cargo.toml --features radio,custom-protocol matrix_tests:: -- --nocapture` | 17 passed; 719 filtered |
| Native UI, registry, and translation integrity | `npm test -- --run src/features/registry.test.ts src/i18n/hardcoded-strings.test.ts src/components/DxChaserView.dxcc.test.tsx` | 206 passed across 3 files |
| Landing 62 affected UI/style integration | `npm test -- --run src/features/registry.test.ts src/i18n/hardcoded-strings.test.ts src/components/DxChaserView.dxcc.test.tsx src/components/TopBar.onair.test.tsx src/styles-palette-roles.test.ts` | 295 passed across 5 files |
| Production UI bundle | `npm run build` | passed |
| Tauri host compile | `cargo check --manifest-path src-tauri/Cargo.toml --features radio,custom-protocol` | passed; optional AI CW model warning only |
| Rust formatting | `cargo fmt --manifest-path Cargo.toml --all -- --check` and `cargo fmt --manifest-path src-tauri/Cargo.toml --all -- --check` | passed |
| Patch whitespace | `git diff --check` | passed |
| Upstream freshness | `bash scripts/sync-mshv-auto-dx-chaser.sh --check` | reviewed pin is current |
| Package-content gate logic | `bash scripts/inspect-dx-chaser-package.test.sh` | fixture suite passed for expanded app, DMG, raw EXE, MSI/NSIS, DEB, and AppImage inputs, including negative split-marker cases |
| macOS model-less package proof | Parent code SHA `5ad7f7734fa1bbf7c0a7d29faef7fe2d99c58e2e`; `NEXUS_ALLOW_MISSING_AICW=1 cargo tauri build --target aarch64-apple-darwin --features radio,custom-protocol --bundles app,dmg`, then `scripts/inspect-dx-chaser-package.sh` on both bundles and a direct `.app` launch | `.app` and `.dmg` were produced, both package inspections passed, and the app launched; the build command then exited 1 because updater signing had a public key but no private key |

The fixture package tests verify the inspector. The macOS proof inspected real local bundles,
but those bundles deliberately omit the DeepCW model and are neither an official signed update
nor evidence for Windows or Linux packaging.

## External evidence still required

| Runtime/release layer | Current evidence state |
|---|---|
| Official model-complete, signed macOS `.app` / `.dmg` produced from the final revision, inspected and launched | PARTIAL EXTERNAL — model-less parent-code bundles were inspected and launched; DeepCW staging and updater signing remain unverified |
| Windows `.exe` / installer produced from this integrated revision, inspected and launched | UNVERIFIED EXTERNAL |
| Linux `.deb` / AppImage produced from this integrated revision, inspected and launched | UNVERIFIED EXTERNAL |
| Account-bound Twilio delivery, including provider receipt | UNVERIFIED EXTERNAL |
| Physical-radio CAT/QSY/TX, decoder, watchdog, and logged-QSO run | UNVERIFIED EXTERNAL |

## Acceptance

All consumed source behaviors at
`kd9taw/mshv-auto-dx-chaser@d960ae22de78940c6be9d95bd4817d233d02ee39`
are `VERIFIED` or `VERIFIED SHARED AUTHORITY`; source-parity acceptance passes. That result
does not prove an official model-complete and signed macOS release or any Windows/Linux package,
nor that a physical radio or Twilio account has completed an end-to-end run. Those release and
runtime claims remain blocked until the external evidence above is collected.
