# Owner acceptance — macOS

The isolated review DMG is:

`src-tauri/target/release/bundle/macos/ShackCQ_Desktop_Nexus_Review_1.14.0-review.2_aarch64.dmg`

SHA-256: `66e2b2f9550d138c05b39f83e571a4f0e9da033d35d3c4e5932ea2d064b3217e`.

It contains **ShackCQ Desktop Nexus Review**, bundle `app.shackcq.desktop.nexusreview`, version `1.14.0-review.2`. It is ad-hoc signed and not notarized, so Gatekeeper rejects normal distribution. It was not launched or installed in this task. Do not open the installed `/Applications/ShackCQ Desktop.app` as evidence for this source tree: that is the preserved `online.shackcq.desktop` version 0.2.2 application.

## Safe review sequence after an artifact is produced

1. Disconnect or power off radios, audio interfaces, keying/PTT adapters and rotators.
2. Confirm the artifact checksum and bundle identifier against its manifest.
3. Open only the isolated review app; create a new isolated review profile.
4. Leave radio, audio, Remote and website connector settings unconfigured.
5. Confirm the app starts in **Basic** mode and exposes Connect, Needed and Settings without digital/TX controls. Switch to **Digi** and visit FT8/FT4, the remaining digital modes, Tempo, RTTY, SSTV, APRS, CW, Phone, Logbook, Propagation, Portable/Field Day, Satellites/Rotator, Memories and Settings. Confirm the mode selection and non-device preferences persist.
6. For decoder review, use an approved bundled/local recording and a null/file sink only. Do not select a live input/output device.
7. Confirm an attempt to arm transmit is refused with the review-build owner-acceptance message.
8. Digi has visible selected-logbook sync status, but this candidate has no operator pairing UI. Do not enter a production token manually. After `SCQ-WEB-AUTH-001` is closed, verify that Digi follows the Web-selected station/logbook authority, imports its ADIF without duplicates on entry and at five-minute intervals, and reports unpaired/error states truthfully.
9. Do not treat Basic website telemetry or automatic completed-contact push as complete until `SCQ-DESKTOP-WIRING-001`, `SCQ-WEB-STATE-001` and `SCQ-WEB-CONTACT-001` are closed with event-driven tests.
10. Use the global Stop control, then quit normally. Confirm no radio/audio/rotator process was started and the existing ShackCQ Desktop profile/data were unchanged.

## Separate authorization still required

Physical audio, CAT readback, PTT, Tune, RF transmit, rotor movement, live provider accounts, signing/notarization, installation and production deployment all remain outside this task. Hardware acceptance must be run mode-by-mode with an owner-approved station safety procedure.
