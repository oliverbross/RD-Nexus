"""Narrow localhost bridge from RD-Nexus into the pinned Neural DX runtime.

This file is copied into the temporary packaging tree. The upstream submodule is
never edited, which keeps a future fast-forward and audit mechanically clear.
"""

import hmac
import os
import threading
import time
from collections import deque
from flask import jsonify, request


def install_nexus_bridge(app, state):
    expected = os.environ.get("NEURAL_DX_NEXUS_TOKEN", "")
    seen = set()
    actions = deque(maxlen=100)
    actions_lock = threading.Lock()

    def authorised():
        supplied = request.headers.get("X-Nexus-Token", "")
        return bool(expected) and hmac.compare_digest(expected, supplied)

    def queue_cluster_line(line):
        """Route an upstream spot-post action through Nexus's validated cluster command."""
        parts = str(line or "").split(maxsplit=3)
        if len(parts) < 3 or parts[0].upper() != "DX":
            return False
        try:
            freq_mhz = float(parts[1]) / 1000.0
        except (TypeError, ValueError):
            return False
        with actions_lock:
            actions.append({
                "type": "postSpot",
                "freqMhz": freq_mhz,
                "call": parts[2].upper(),
                "comment": parts[3] if len(parts) == 4 else "",
            })
        return True

    state.cluster_send_line = queue_cluster_line

    @app.post("/api/nexus/sync")
    def nexus_sync():
        if not authorised():
            return jsonify({"ok": False, "error": "unauthorised"}), 401
        payload = request.get_json(silent=True) or {}
        station = payload.get("station") or {}
        call = str(station.get("callsign") or "").strip().upper()
        grid = str(station.get("grid") or "").strip().upper()
        if call:
            state.MY_CALL = call
        if grid:
            state.user_qra = grid
            state._update_location_from_user_qra()

        incoming = payload.get("spots")
        if isinstance(incoming, list):
            state.spots_buffer.clear()
            for spot in incoming[-state.spots_buffer.maxlen:]:
                if not isinstance(spot, dict) or not spot.get("dx_call"):
                    continue
                state.spots_buffer.append(spot)
                identity = (spot.get("dx_call"), spot.get("band"), int(spot.get("timestamp", 0) // 60))
                if identity not in seen:
                    seen.add(identity)
                    try:
                        state.predictor.record_spot(spot, is_watchlist=spot.get("dx_call", "").upper() in state.watchlist)
                    except Exception:
                        pass
                    if state._ANALYTICS_OK:
                        try:
                            state._analytics_mod.record_spot(spot)
                        except Exception:
                            pass
            if len(seen) > 20000:
                seen.clear()

        qsos = payload.get("qsos")
        if isinstance(qsos, list):
            confirmed_calls = set()
            confirmed_dxcc = set()
            worked_dxcc = set()
            dxcc_by_band = {}
            for qso in qsos:
                if not isinstance(qso, dict):
                    continue
                call = str(qso.get("call") or "").upper()
                entity = str(qso.get("entity") or "")
                band = str(qso.get("band") or "")
                confirmed = bool(qso.get("confirmed"))
                if entity:
                    worked_dxcc.add(entity)
                if confirmed:
                    if call:
                        confirmed_calls.add(call)
                    if entity:
                        confirmed_dxcc.add(entity)
                        dxcc_by_band.setdefault(band, set()).add(entity)
            with state.lotw_lock:
                state.lotw_data.update({
                    "confirmed_calls": confirmed_calls,
                    "confirmed_dxcc": confirmed_dxcc,
                    "worked_dxcc": worked_dxcc,
                    "dxcc_by_band": dxcc_by_band,
                    "total_qso": len(qsos),
                    "total_confirmed": len(confirmed_calls),
                })
                state.lotw_session["logged_in"] = bool(qsos)
                state.lotw_session["last_sync"] = time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime())

        return jsonify({"ok": True, "spots": len(state.spots_buffer), "qsos": len(qsos or [])})

    @app.get("/api/nexus/status")
    def nexus_status():
        if not authorised():
            return jsonify({"ok": False, "error": "unauthorised"}), 401
        return jsonify({"ok": True, "embedded": True, "spots": len(state.spots_buffer)})

    @app.get("/api/nexus/actions")
    def nexus_actions():
        if not authorised():
            return jsonify({"ok": False, "error": "unauthorised"}), 401
        with actions_lock:
            pending = list(actions)
            actions.clear()
        return jsonify({"ok": True, "actions": pending})
