#!/bin/sh
set -eu

# Hamlib's Dummy rig is independent fake hardware: it keeps its own frequency/mode
# state and returns that state over the real rigctld protocol. Loopback only; model 1
# cannot access serial, audio, PTT, or RF hardware.
exec /opt/homebrew/bin/rigctld -m 1 -T 127.0.0.1 -t 4599 -v
