# Nexus bridge contract

The bridge exists only when OpenHamClock is loaded with `?nexus=1` inside the
Nexus-owned iframe. Both sides validate the message source; Nexus also validates
the exact loopback origin selected by the native supervisor.

Nexus sends a small, frequently refreshed `state` message containing:

- `station`: callsign and grid;
- `rig`: connection, frequency in Hz, mode, and PTT state;
- `wsjtx`: Nexus's current decode slot and radio status in OpenHamClock's
  WSJT-X shape, so no second UDP listener competes for the station feed;
- `spots`: the current Nexus DX-cluster/RBN firehose mapped to OpenHamClock's
  path feed;
- `rotator`: current azimuth and liveness.

The logbook is transferred separately as revisioned `logbook:begin`, bounded
`logbook:chunk`, and `logbook:end` messages. A chunk contains at most 500 rows
and 512 KiB as a complete encoded bridge message. A single encoded row is
limited to 510 KiB, reserving 2 KiB for the message envelope, so a
67,000-contact station does not freeze the UI or exceed webview message limits.
The iframe requests a fresh
snapshot on startup; subsequent snapshots are sent after log changes, not on
the radio/spot refresh cadence.

OpenHamClock sends narrowly typed requests back:

- `logbook:add`, `logbook:addMany`, `logbook:update`, `logbook:remove`, and
  `logbook:clear`;
- `rig:setFreq`, `rig:setMode`, and `rig:setPTT`;
- `cluster:post`;
- `rotator:turn` and `rotator:stop`;
- `ready`, requesting an immediate state snapshot after frame load.

Every request is executed through the existing Nexus command. The bridge does
not bypass license, transmit, CAT, rotator, log consistency, or unassisted-mode
guards. In particular, embedded PTT uses the Nexus broker arbiter and the Ham
Clock view exposes an always-reachable STOP TX control while keyed. Both the
state sent to OpenHamClock and STOP TX visibility use Nexus's canonical on-air
predicate, covering FT slot transmission, broker/manual PTT ownership, and CAT
rig-key readback rather than relying on the FT-only `transmitting` flag.

Nexus is the system of record for mirrored contacts. OpenHamClock stores the
explicit Nexus ID in each mirrored row and in a durable local ledger. That
binding—not a callsign/time guess—drives updates and deletions across iframe or
application restarts. Native OpenHamClock-only contacts remain intact until
their own operation is sent to Nexus. Arbitrary ADIF fields are carried in the
record's extras map so the bridge is not a lossy fixed-field projection.

The initial mirror is paged and transferred in bounded chunks. Later native
changes use a bounded revision delta instead of re-reading or retransmitting the
whole log. Snapshot and delta commits are serialized, acknowledged, and advance
the mirrored rows, binding ledger, and revision in one IndexedDB transaction.
Stale revisions are ignored and gaps request a fresh bounded snapshot, so a
crash cannot publish a new ledger beside old rows or move the durable revision
backwards.
