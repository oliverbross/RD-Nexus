# Groups.io parity ledger — 2026-09-27

## Scope and reviewed revisions

- Source product: ShackCQ in `/Users/oliver/Documents/Projects/RigWeave/f17-resolution-20260923`.
- Source revision reviewed: `636d25c` (`codex/f17-resolution-20260923`); refreshed `origin/main` was `4fdb14c`.
- Source implementation reviewed: `services/application/src/groupsio/service.ts`, `services/application/src/groupsio/routes.ts`, `web/src/groupsio/GroupsIoWorkspace.tsx`, `web/src/groupsio/groupsio.css`, the Groups.io node/web tests, and the two Groups.io QA scripts.
- RD-Nexus base reviewed and merged initially at `68d89dd3`, then refreshed and merged again at `kd9taw/Nexus` `upstream/main` `35ec66ca` (Landing 61).
- RD-Nexus Groups.io implementation series: `8ae2d3b0`, `8322c3ef`, refreshed-upstream merge `12431d14`, evidence `eb56ff2b`, crash-safety `d731ad28`, and nine-point remediation `b2aec7c1`; the current review-fix revision is recorded by the branch HEAD submitted at the final gate.
- Provider contract carried forward: `https://groups.io/api/v1`, revision marker `2026-07-31`.

`COMPLETE` below means the ShackCQ behavior has an RD-Nexus implementation and local executable or rendered evidence. `EXTERNAL` means implementation is present but proof requires an operator-owned Groups.io account/key and is deliberately not represented as local proof.

## Behavior mapping

| Capability | ShackCQ source behavior | RD-Nexus implementation | Evidence | Status |
|---|---|---|---|---|
| Navigation and screen | Groups.io workspace | `ModeNav.tsx`, `App.tsx`, `GroupsIoView.tsx`; entry immediately follows Memories | rendered desktop screenshot `groupsio-screen.png`; navigation registry tests | COMPLETE |
| Settings surface | Provider/account and sync policy settings | `SettingsPanel.tsx`, settings registry, `GroupsIoSettings.tsx` | rendered `groupsio-settings.png`; component and hardcoded-string tests | COMPLETE |
| Three-pane workspace | Memberships, topics, messages with cached empty/offline states | `GroupsIoView.tsx`, `groups-io.css` | `GroupsIoView.test.tsx` offline three-pane case | COMPLETE |
| Credential entry | User supplies an API key; password is never requested | `groups_io_connect`; password input is cleared after connect | settings component test; Rust store serialization negative control | COMPLETE |
| Credential storage | Secret must not enter settings/cache/logs | `keyring` service `tempo`, account `groupsio-api-key`; `Store` has no credential field | Rust `defaults_are_foreground_only_and_do_not_contain_a_credential`; UI stored-key negative test | COMPLETE |
| Cross-platform secret backend | Keychain / Credential Manager / Secret Service | `keyring` Apple, Windows and sync-secret-service features | dependency/features inspection; platform package build remains required per target | COMPLETE |
| Connect/verify/disconnect | Verify before persistence; remove only credential on disconnect | `groups_io_connect`, `groups_io_disconnect` | Rust error paths and settings UI actions | COMPLETE |
| Membership discovery | Paginated `/getsubs`; repeated-token stop; permission enrichment | `sync_memberships_with`, `/getperms`, `/getsinglefeed` | parser/unit coverage; account call | EXTERNAL |
| Membership policy | enabled/inherit/lookback/max/attachments/retention per group | `GroupsIoSettings`, `GroupsIoGroupPolicy`, `effective_policy` | bulk-selection component test; settings bound test | COMPLETE |
| Bulk policy edit | apply and reset selected memberships | `selectedGroups`, `applySelected`, `resetSelected` | `GroupsIoSettings.test.tsx` | COMPLETE |
| Topics | `/gettopics`, cached listing, older-page cursor | `groups_io_load_older_topics`, `topic_tokens` | pagination component test; account call | COMPLETE / EXTERNAL network proof |
| Messages and threads | `/gettopic`/`getmessages`, chronological thread, more-page cursor | `groups_io_load_more_messages`, `message_tokens` | pagination component test; account call | COMPLETE / EXTERNAL network proof |
| Local search | Offline body/subject/sender search | `groups_io_search(online=false)` | UI search state and native implementation inspection | COMPLETE |
| Online search/open result | `/searcharchives`; cache returned results before opening | `groups_io_search(online=true)` and store upsert | implementation inspection; account call | EXTERNAL |
| Offline cache | Read memberships/topics/messages with no credential | persisted `Store`; `groups_io_status`; offline UI boundary | offline component test and store round-trip test | COMPLETE |
| Retention and caps | Per-group lookback, retention/max-message pruning and draft retention | `effective_policy`; cutoff filtering and bounded pruning in `sync_group_with`; validated bounds | Rust lookback/settings-bound and store tests | COMPLETE |
| Refresh on open | Refresh only when connected/enabled and stale | guarded `refreshedOnOpen` effect in `GroupsIoView` | component/source inspection | COMPLETE |
| Foreground interval | Consume configured minutes only while the screen/app is mounted; no daemon poller | cancellable `setInterval` effect using `foregroundRefreshMinutes`; cleanup on unmount | component/source inspection; no native background scheduler exists | COMPLETE |
| Muted groups | Skip muted groups unless include-muted is enabled; preview can mute | native sync filter; preview mute action | implementation inspection | COMPLETE |
| New-message preview | Optional foreground preview with open/dismiss/mute; full foreground refresh starts a fresh ID set | `showNewMessagePreview`, source-order `newMessageIds`, preview aside | native ordering/reset test and rendered/component source | COMPLETE |
| Local drafts | save/edit/queue and survive restart | persisted `Store.drafts`; composer and queue actions | store round trip; component UI | COMPLETE |
| Server drafts | list/page/open local copy/delete | `/getdrafts`, `/deletedraft`; draft dialog | component UI; account call | EXTERNAL |
| Server draft attachments | list and explicitly delete | `/getattachments`, `/deleteattachment`; draft dialog controls | native/UI implementation; account call | EXTERNAL |
| Explicit posting | create/update draft, upload, review confirmation, foreground `/postdraft` | `groups_io_send_draft(confirmed)` and confirmation UI | native state-machine tests/source; account send deliberately not exercised | EXTERNAL |
| Delivery ambiguity | Only documented 400/401/403/422 rejection is retryable; missing/conflict/undocumented 4xx, timeout/rate-limit/5xx, transport or unreadable success is `delivery_unknown`; never auto-retry | explicit whitelist in `classify_final_post_status`; UI refreshes native state on failure | Rust status-boundary tests cover 400/401/403/422, 404/409, timeout/rate-limit and representative 5xx; undocumented 418/499 are persisted and reloaded as reconciliation-required; UI stale-state regression | COMPLETE |
| Crash-safe final posting | Durably commit a stable final-post attempt before `/postdraft`; rename/parent-sync uncertainty must block submission and restart as `delivery_unknown` | `final_post_attempt_id`, `posting`, required durable replacement, startup recovery | Rust injected parent-sync failure proves provider path remains unreachable; restart/provider-success/receipt-failure tests | COMPLETE on tested POSIX host / EXTERNAL Windows execution |
| Duplicate prevention | No retry after an interrupted or ambiguous final POST; operator must reconcile as delivered or confirmed-not-delivered | backend rejects edit/queue/delete/send while uncertain; explicit reconciliation command and UI controls | Rust recovery tests and `GroupsIoView.test.tsx` uncertain-delivery case | COMPLETE |
| Outbox receipts | Persist posted, moderated and uncertain receipts | `Store.outbox`, drafts/outbox dialog | store persistence path; UI | COMPLETE |
| Concurrent persistence | Independent mutations cannot overwrite one another; replacement is atomic and its durability gate completes before success | transaction lock; POSIX rename plus required parent-directory fsync; Windows temp attributes normalized before `MoveFileExW(REPLACE_EXISTING | WRITE_THROUGH)` after temp-file sync | executed POSIX concurrent mutation, 64 replacements and injected sync failure; Windows primitive and conditional test verified by source inspection only, not executed on this macOS host | COMPLETE on tested POSIX host / EXTERNAL Windows execution |
| Outgoing attachments | private local staging, cap, SHA-256 identity, multipart upload, remove | `groups_io_add_attachment`, `groups_io_remove_attachment`, multipart send | native safety tests; account upload | COMPLETE / EXTERNAL network proof |
| Incoming attachments | discover, preview, download, optional cache | incoming attachment commands and dialog | component pagination/attachment test; account download | EXTERNAL |
| Attachment failure isolation | Auto-download failure never rolls back a complete message page | best-effort `cache_incoming_for_message` call after message upsert | native implementation inspection | COMPLETE |
| Attachment path safety | Provider/persisted IDs never become path components | fixed SHA-256 cache key over namespace identity; separate provider ID | traversal/absolute/backslash/Unicode/poisoned-store Rust test | COMPLETE |
| Download bounds | Reject Content-Length overflow and stop streaming at limit+1; also cap chunked/missing/lying length | `read_limited`, `read_limited_response`; caps for JSON, attachments, previews and official ZIP | Rust missing/lying/declared length test | COMPLETE |
| Bearer isolation | Redirects disabled and attachment-host requests never receive API Authorization | `Client` redirect policy `none`; `attachment_get` has no bearer | Rust request-header negative test | COMPLETE |
| Archive sync | bounded descending pages, date cutoff, cursor resume, duplicate replacement | `groups_io_archive`, `ArchiveState` | native implementation and UI status | COMPLETE / EXTERNAL network proof |
| Archive pause/cancel | foreground cancel, preserve completed pages/cursor, and never queue a later start behind the active transaction | pause signals before lock; archive uses `try_store_transaction` | native queued-start test and UI implementation | COMPLETE |
| Official archive | permission-gated `/downloadarchives` ZIP with hard stream cap | `groups_io_official_archive`; conditional UI action | native implementation; account permission/download | EXTERNAL |
| Clear cache | remove downloaded/cache files, preserve drafts/outbox/key | confirmed Settings action; `groups_io_clear_cache(false)` | component/source inspection | COMPLETE |
| Delete all local | remove ordinary downloads/drafts/receipts/staged files; preserve credential and any uncertain attempt plus its staged attachments until reconciliation | confirmed Settings action; protected-ID retention in `groups_io_clear_cache(true)` | native protection test and component/source inspection | COMPLETE |
| Destructive remote actions | Server-draft and server-attachment deletion require confirmation | localized confirmation controls in `GroupsIoView` | component/source inspection | COMPLETE |
| Error and empty states | offline, empty group/topic/message/search/draft/attachment, busy and archive state | localized `GroupsIoView`/`GroupsIoSettings` states | hardcoded-string guard and component tests | COMPLETE |
| Accessibility | labelled region/search group, switch roles, current selection, live summary/preview, Radix dialog title+description | Groups.io components | hardcoded-string and component tests; rendered browser inspection | COMPLETE |
| Localization | all operator prose through the catalog; core German/Spanish/French/Japanese Groups.io controls localized | `en.ts`, `de.ts`, `es.ts`, `fr.ts`, `ja.ts`; migrated-surface guard | i18n hardcoded and placeholder/completeness tests | COMPLETE |
| Compact layout | panes stack into a vertically scrollable workspace before zoomed min-widths can hide the message pane; controls also collapse at narrow widths | 920px and 560px container rules in `groups-io.css` | pinned 175% zoom/compact browser inspection plus stylesheet | COMPLETE |

## Verification record

- `npm run build`: PASS after integration, upstream merge and crash-safety fixes.
- Post-upstream UI selection (Groups.io components, i18n guards/runtime, feature registry): PASS (258 tests across 7 files).
- Rust Groups.io tests with the CI-required `radio` feature: PASS (22 tests; 719 filtered), including HTTP ambiguity boundaries, remote-route immutability, lookback, immutable uncertain attempts, archive start rejection, preview ordering, concurrent replacements, injected parent-directory-sync failure with provider-call suppression, and final-post crash/recovery boundaries. The conditional Windows write-through execution and cleared-temporary-attribute assertions require a Windows runner and are not represented as executed here.
- `cargo fmt --manifest-path src-tauri/Cargo.toml --all -- --check`: PASS.
- A full UI-suite diagnostic run also exercised the Groups.io tests successfully, but it was not a repository-wide pass: eight failures were reported in unrelated stop-line, CW/phone cockpit density/keyer and phone-scope tests under concurrent shared-host builds. It is not counted as Groups.io acceptance evidence.
- Real rendered browser evidence:
  - `/Users/oliver/.codex/visualizations/2026/09/27/01a0e2aa-3b8f-7b00-868d-80aba8311f7e/groupsio-screen.png`
  - `/Users/oliver/.codex/visualizations/2026/09/27/01a0e2aa-3b8f-7b00-868d-80aba8311f7e/groupsio-settings.png`
  - `/Users/oliver/.codex/visualizations/2026/09/27/01a0e2aa-3b8f-7b00-868d-80aba8311f7e/groupsio-175-percent.png` (1050×800 viewport, CSS zoom 175%; groups/topics/messages stacked and fully reachable)
- The repository browser-probe wrapper could not find `google-chrome` on `PATH`; the installed Google Chrome application was instead used through the browser automation harness. This is not represented as a wrapper-script pass.

## Explicit external evidence boundary

No operator API key was supplied to this worktree. Consequently, real account membership discovery, permission reads, online archive search, remote draft mutation, attachment transfer, final posting/moderation, and official ZIP download remain `EXTERNAL`. Local mocks, parser tests and rendered screenshots do not convert those rows into live provider proof. No release should claim account-bound acceptance until an operator runs those cases with their own key and inspects the resulting Groups.io account state.
