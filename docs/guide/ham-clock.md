# Ham Clock

Ham Clock brings the complete OpenHamClock dashboard into Nexus. Open it from
**Ham Clock** at the top of the left rail when you want a station-wide view of
maps, propagation, spots, satellites, space weather, clocks and emergency
information without running a second application.

## Before you start

- Set your callsign and grid in
  [Settings ▸ Station ▸ Operator & Radio](settings-reference.md#operator--radio).
- Connect a radio only if you want Ham Clock controls to tune or key it; the
  dashboard and its internet-fed information can still be used without CAT.
- Configure any internet service, cluster, rotator or station accessory that a
  particular OpenHamClock panel needs before relying on that panel's data.

## The tour

The main frame is the full OpenHamClock workspace. Its layouts and panels cover
the world and azimuthal maps, propagation, DX cluster, satellites, solar and
space weather, UTC clocks, station information, emergency information and the
other tools supplied by the pinned OpenHamClock version.

Nexus starts the bundled dashboard when this section is opened. The status area
shows while it is starting and offers **Restart OpenHamClock** if the local
service stops. Its layouts, panel choices and OpenHamClock settings persist in
the same per-user application data area as the rest of Nexus.

Station state is shared with the frame: callsign and grid, current frequency and
mode, transmit state, live spots, decoder status and rotator position. The Nexus
logbook remains the canonical record. Contacts created or changed on either side
are mirrored with their full fields and stable identity, so an edit or deletion
does not depend on guessing from callsign and time.

When any Nexus-controlled transmission is on the air, **STOP TX** stays above
the dashboard. It invokes the same global stop used elsewhere in Nexus; it is
not an OpenHamClock-only un-key button.

## Core workflows

### Open and arrange the dashboard

1. Select **Ham Clock** in the left rail. The status changes from starting to the
   embedded dashboard.
2. Choose an OpenHamClock layout or arrange its panels. The new arrangement is
   retained for the next visit.
3. Open the panels you need. Each online panel reports its own missing service
   or unavailable data rather than inventing a value.

### Tune or turn the station from Ham Clock

1. Select a frequency, mode or rotator target in the dashboard. Nexus receives
   the request through its normal station controls.
2. Check the Nexus dial, mode or rotator indication for the resulting state.
3. If a transmit request is appropriate, enable transmit in Nexus first. Ham
   Clock cannot grant itself transmit authority.
4. Use **STOP TX** whenever the station must be made safe. Nexus disarms and
   stops every transmission path it controls.

### Keep the logbook in step

1. Add or edit a contact in Ham Clock or in the Nexus logbook.
2. Open the other view and confirm the same contact and fields appear there.
3. If Ham Clock asks to clear the log, review the second Nexus confirmation.
4. Choose **Back up and clear** only when the whole Nexus logbook should be
   cleared. Nexus saves an ADIF backup before deleting the contacts.

### Recover an unavailable dashboard

1. Wait for the starting message to finish; the first launch may take longer
   while the bundled service opens its data store.
2. If the unavailable message remains, select **Restart OpenHamClock**.
3. Reopen the section. Saved layouts and the contact binding are retained across
   the restart.
4. If it still does not start, restart Nexus and check that security software has
   not quarantined a file inside the installed application.

## Honest limits

The bundled dashboard means no separate OpenHamClock, Node.js, Docker service or
browser launch is required. It does not make external feeds work without an
internet connection, provider availability or the configuration those feeds
require. Propagation, solar and satellite panels are predictions or third-party
observations; seeing them is not proof that a path is open at your station.

Automated tests verify the local bridge protocol, bounded log transfer, restart
recovery logic and Nexus transmit guards. Package inspection verifies that the
required resources are assembled. Neither substitutes for a separate,
SHA-bound packaged-runtime receipt, nor proves a physical-radio QSY or contact,
a live rotator, every upstream data provider, a signed and notarized public
installer, or that an external microphone, VOX circuit or front-panel PTT
outside Nexus control has been released.

## Related guides

- [Connect — map + propagation](connect.md) covers Nexus-native maps and path
  forecasts.
- [Spots](spots.md) explains the shared cluster and RBN traffic.
- [Satellites](satellites.md) covers Nexus-native passes and rotator workflows.
- [Logbook & QSL](logbook-qsl.md) explains the canonical contact record.
- [Settings reference](settings-reference.md) covers station, radio, feeds and
  feature visibility.
