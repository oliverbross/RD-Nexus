# Neural DX Watcher source permission

On 2026-09-27, the RD-Nexus maintainer confirmed that the original Neural DX Watcher developer
gave specific permission to use the project in ShackCQ.com and the maintainer's other software
projects. That attestation authorizes continued private integration work, but the exact written
grant terms have not been archived in this repository.

The public upstream README also labels the code “MIT”, but the audited v12.7 tree contains no
licence file and no per-file licence headers. This record does not convert the private grant into
a general public licence. Preserve the upstream author credit, repository URL and exact source pin
in every distributed package, and re-check the upstream tree when advancing the pin.

**Release gate:** `BLOCKED_PENDING_LICENSE_EVIDENCE`. Do not publicly push a vendored source
payload or publish DMG, EXE, AppImage, or DEB artifacts containing Neural DX until the written
grant establishes modification, redistribution, and sublicensing on terms compatible with
RD-Nexus's GPL-3.0-only distribution. A README label alone is not that evidence. Local builds and
private validation may continue; clearing this gate requires retaining the actual grant evidence
and adding `docs/neural-dx/REDISTRIBUTION_APPROVED.md` with the completed compatibility review.

Private maintainer integration builds must opt in explicitly with
`NEURAL_DX_PRIVATE_BUILD=1`; that exception is never set by the public release workflow.
The public-release workflow fails closed while that review record is absent.
