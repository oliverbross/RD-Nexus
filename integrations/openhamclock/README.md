# OpenHamClock integration

RD-Nexus pins the complete upstream OpenHamClock repository as the
`third_party/openhamclock` submodule. `scripts/stage-openhamclock.mjs` verifies
that pin, builds the full React frontend and Express backend, applies only the
integration adaptations documented in the script, and creates a
self-contained target executable. Tauri bundles that executable, the upstream
MIT license, generated dependency notices, the verified embedded Node.js
runtime license, and provenance into the normal RD-Nexus installer/app bundle.

The operator installs and opens one application. On the first visit to Ham
Clock, Nexus starts and owns the companion process, binds it to loopback, gives
it a private data directory, displays it in that view, and stops it when Nexus
exits. The lazy start keeps OpenHamClock from consuming resources or opening
its independent data feeds when the screen is unused. No system Node.js,
Docker, separate installer, or separately launched browser is required.

PTT messages originating inside the embedded upstream UI remain scoped to the
OpenHamClock broker owner: key and un-key requests go through Nexus's broker
safety arbiter. The always-visible **STOP TX** overlay belongs to Nexus and uses
Nexus's canonical global halt, so it disarms and stops every Nexus-controlled
owner (FT slots, manual or broker PTT, Tune, voice/CW, continuous keyboard modes,
SSTV, and queued transmissions). It cannot release a front-panel microphone,
VOX, or another physical/external keying path that Nexus does not own; that
hardware must be released at its source.

To update upstream:

1. Run `node scripts/sync-openhamclock.mjs`.
2. Review every upstream commit and the generated license report.
3. Run `node scripts/stage-openhamclock.mjs --target <target>` for every release target.
4. Run OpenHamClock's tests and the RD-Nexus gates/browser probe.
   For a native staged target, run `node scripts/smoke-openhamclock.mjs`.
5. Generate `integrations/openhamclock/PARITY-LEDGER.md` from the staged
   inventory and locked dependency closure, then review every non-PASS row.
6. Update `NOTICE` and release notes if upstream attribution or behavior changed.

The sync command never commits, pushes, or publishes. A changed integration
anchor fails loudly so an upstream update cannot silently drop the loopback or
embedding boundary.

OpenHamClock's git-based self-updater is intentionally replaced by the
RD-Nexus release/update boundary: an installed signed application cannot safely
rewrite a nested bundled executable. The sync command has positive and negative
controls proving that it detects a newer upstream commit without modifying the
pin unless `--apply` is explicitly supplied and reviewed.
