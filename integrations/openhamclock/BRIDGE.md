# Nexus bridge contract

The bridge exists only when OpenHamClock is loaded with `?nexus=1` inside the
Nexus-owned iframe. Both sides validate the message source; Nexus also validates
the exact loopback origin selected by the native supervisor.

Nexus sends a `state` message containing:

- `station`: callsign and grid;
- `rig`: connection, frequency in Hz, mode, and PTT state;
- `wsjtx`: Nexus's current decode slot and radio status in OpenHamClock's
  WSJT-X shape, so no second UDP listener competes for the station feed;
- `logbook`: the complete Nexus-authoritative log represented in OpenHamClock's
  ADIF-shaped fields, including the opaque Nexus row and edit keys;
- `spots`: the current Nexus DX-cluster/RBN firehose mapped to OpenHamClock's
  path feed;
- `rotator`: current azimuth and liveness.

OpenHamClock sends narrowly typed requests back:

- `logbook:add`, `logbook:addMany`, `logbook:update`, `logbook:remove`, and
  `logbook:clear`;
- `rig:setFreq`, `rig:setMode`, and `rig:setPTT`;
- `cluster:post`;
- `rotator:turn` and `rotator:stop`;
- `ready`, requesting an immediate state snapshot after frame load.

Every request is executed through the existing Nexus command. The bridge does
not bypass license, transmit, CAT, rotator, log consistency, or unassisted-mode
guards. Nexus remains the system of record and replaces OpenHamClock's browser
log mirror after any change.
