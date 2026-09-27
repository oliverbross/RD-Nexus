# OpenHamClock license audit

Reviewed upstream revision 6cfb6b2d8011828a8067e6d9f3659044168cbef1
(OpenHamClock 26.8.0).

- The upstream project is MIT licensed. Its complete license is retained in the
  submodule and copied into every packaged application as
  OPENHAMCLOCK-LICENSE.txt.
- src/plugins/layers/useZones.js identifies HB9HIL zone-layer code as MIT.
- src/utils/orbit.js carries the full MIT notice for Florian Mauracher
  (2018).
- The npm dependency graphs are pinned by the upstream and integration
  package-lock.json files. scripts/check-openhamclock-licenses.mjs rejects a
  missing or unreviewed license expression in either graph.
- The self-contained executable embeds the `@yao-pkg/pkg` Node.js 22.23.2
  runtime. The official Node.js license is fetched from the versioned Node
  repository, SHA-256 verified, and shipped as NODE-RUNTIME-LICENSE.txt.
- Map and tile attribution remains rendered by OpenHamClock itself. The
  integration does not remove or cover upstream attribution surfaces.

This audit must be repeated when upstream.json moves to a new revision.
