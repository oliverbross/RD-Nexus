#!/bin/sh
set -eu

# Hamlib's Dummy rotor keeps its own azimuth/elevation state and exposes the real
# rotctld protocol on loopback. It has no serial device and cannot move hardware.
exec /opt/homebrew/bin/rotctld -m 1 -T 127.0.0.1 -t 4600 -v
