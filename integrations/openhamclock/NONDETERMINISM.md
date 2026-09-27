# OpenHamClock executable reproducibility boundary

Repeated `@yao-pkg/pkg` builds for the same OpenHamClock commit, version,
target, and executable size have produced different SHA-256 values. This is a
binary-packaging reproducibility limitation, not source drift:

- staging refuses to run unless the submodule HEAD exactly matches
  `upstream.json`;
- every run copies that pinned tree while excluding prior `node_modules` and
  `dist` output, then installs from the pinned lockfiles and rebuilds assets;
- the per-run receipt records the reviewed repository, commit, version,
  platform/architecture, executable format, size, and actual SHA-256;
- the package checker requires the receipt copied into the application bundle
  and the bundled executable to match the staged artifact byte-for-byte.

The remaining variation is therefore inside the generated single-executable
container. No committed hash is described as canonical. A release artifact is
accepted only against its own same-run receipt; signing/notarization may update
the executable first, in which case the receipt is regenerated before Tauri
bundles and signs the enclosing application.
