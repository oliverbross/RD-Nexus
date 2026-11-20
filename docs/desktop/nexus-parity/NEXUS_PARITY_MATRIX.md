# ShackCQ Desktop Nexus parity matrix

Result: **NOT COMPLETE**

Baseline: Nexus v1.14.0, commit `12efe3d2970729465add40d62cecd76bbaf5b63f`. The annotated release tag object begins `4146ee1f` and the release was published at `2026-09-20T20:48:49Z`.

This ledger treats the pinned, unabridged Nexus source as the product base. Nexus radio, multi-radio, Hamlib, rotator/satellite, digital/audio and logbook implementations ship unchanged unless a row names a narrow integration substitution. “Present in source” is not counted as packaged or hardware verification.

| View | IDs | Result |
| --- | --- | --- |
| Nexus operating/Digi parity | NX-FT8-001 through NX-APRS-008 | 2 software-verified and 6 implemented/unverified; none physically verified; logbook evidence is not per-mode or automatic-QSO proof |
| Whole-product Nexus coverage | NX-RADIO-009 through NX-LIFECYCLE-019 | 11 implemented, not packaged/physically verified |
| Remote/update integration | NX-REMOTE-020, NX-UPDATE-021 | 1 partial, 1 missing |
| ShackCQ substitutions | SCQ-PACKAGE-022 through SCQ-CONTACT-027 | 5 packaged-Mac verified, 1 notices/inventory row unverified |
| Upstream limitation | NX-FOX-028 | upstream not implemented |
| Product modes and logbook | SCQ-MODES-029, SCQ-LOGBOOK-030 | packaged-Mac verified for the agreed isolated workflows |
| Platform artifact | SCQ-MAC-029 | packaged-Mac verified; notarization and Windows remain separate |

## Status counts

There are 31 rows total: 1 `MISSING`, 1 `PARTIAL`, 18 `IMPLEMENTED_UNVERIFIED`, 2 `VERIFIED_SOFTWARE`, 8 `VERIFIED_PACKAGED_MAC`, 0 packaged-Windows, and 1 `UPSTREAM_NOT_IMPLEMENTED`.

The machine-readable JSON is authoritative for every required field and blocker. The remaining product-level gaps are:

- `SCQ-WEB-REMOTE-001`: implement or explicitly retire the default-off Nexus Remote `/api/remote/*` protocol for ShackCQ.
- `SCQ-DIST-UPDATE-001`: approved ShackCQ update signing and distribution plumbing.
- `SCQ-LICENSE-023`: the exact Review 6.8 inventory is recorded and includes the CQ-zone licence; reproducible staging and source correspondence for the missing Hamlib runtime remain open.
- Windows packaging, macOS notarization/stapling, official WaveLogGate guest validation and later owner-authorized hardware/TX acceptance remain separate gates.

The Review 6.7 macOS candidate has packaged, signed and isolated runtime evidence. Its six native Basic/Digi × logbook-authority rows remain accepted 6/6. The three official WaveLogGate rows remain explicitly NOT RUN pending an authorized bootable guest. The unsigned local Review 6.8 workflow-repair bundle was produced on 2026-09-28 and inventory-recorded, but was not launched or runtime-verified after the user stopped further verification. No row claims notarization, Windows support, physical-radio/on-air acceptance or production deployment.
