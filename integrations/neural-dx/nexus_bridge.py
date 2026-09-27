"""Narrow localhost bridge from RD-Nexus into the pinned Neural DX runtime.

This file is copied into the temporary packaging tree. The upstream submodule is
never edited, which keeps a future fast-forward and audit mechanically clear.
"""

import hmac
import json
import os
import re
import threading
import time
from collections import deque
from pathlib import Path
from flask import jsonify, request
from tools.log_meta_analyzer import build_summary, parse_log


def install_nexus_bridge(app, state):
    app.config["MAX_CONTENT_LENGTH"] = 16 * 1024 * 1024
    expected = os.environ.get("NEURAL_DX_NEXUS_TOKEN", "")
    seen = set()
    actions = deque(maxlen=100)
    actions_lock = threading.Lock()
    recent_actions = {}
    upstream_qsos = {
        "confirmed_calls": set(),
        "confirmed_dxcc": set(),
        "worked_calls": set(),
        "worked_dxcc": set(),
        "dxcc_by_band": {},
        "total_qso": 0,
        "total_confirmed": 0,
    }
    last_merged_qsos = None
    upstream_cluster_send_line = state.cluster_send_line
    upstream_spot_capacity = state.spots_buffer.maxlen
    state.spots_buffer = deque(state.spots_buffer, maxlen=upstream_spot_capacity * 2)

    def authorised():
        supplied = request.headers.get("X-Nexus-Token", "")
        return bool(expected) and hmac.compare_digest(expected, supplied)

    def nexus_cluster_line(line):
        """Preserve upstream cluster TX, falling back to Nexus's validated command."""
        if upstream_cluster_send_line(line):
            return True
        parts = str(line or "").split(maxsplit=3)
        if len(parts) < 3 or parts[0].upper() != "DX":
            return False
        try:
            freq_mhz = float(parts[1]) / 1000.0
        except (TypeError, ValueError):
            return False
        call = parts[2].upper()
        comment = parts[3] if len(parts) == 4 else ""
        if not (0.1 <= freq_mhz <= 1300.0):
            return False
        if not re.fullmatch(r"[A-Z0-9/]{3,16}", call):
            return False
        if len(comment) > 120 or any(ord(char) < 32 and char not in "\t" for char in comment):
            return False
        signature = (round(freq_mhz, 6), call, comment)
        now = time.monotonic()
        with actions_lock:
            for key, recorded in list(recent_actions.items()):
                if now - recorded > 30:
                    recent_actions.pop(key, None)
            if signature in recent_actions:
                return True
            recent_actions[signature] = now
            actions.append({
                "type": "postSpot",
                "freqMhz": freq_mhz,
                "call": call,
                "comment": comment,
            })
        return True

    state.cluster_send_line = nexus_cluster_line

    @app.post("/api/nexus/sync")
    def nexus_sync():
        if not authorised():
            return jsonify({"ok": False, "error": "unauthorised"}), 401
        payload = request.get_json(silent=True) or {}
        if not isinstance(payload, dict):
            return jsonify({"ok": False, "error": "invalid payload"}), 400
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
            if len(incoming) > 5000:
                return jsonify({"ok": False, "error": "too many spots"}), 413
            original = [spot for spot in state.spots_buffer if spot.get("nexus_source") != "shared"]
            state.spots_buffer.clear()
            state.spots_buffer.extend(original[-upstream_spot_capacity:])
            for spot in incoming[-upstream_spot_capacity:]:
                if not isinstance(spot, dict) or not spot.get("dx_call"):
                    continue
                spot = dict(spot)
                spot["nexus_source"] = "shared"
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
        qso_summary = payload.get("qsoSummary")
        if isinstance(qso_summary, dict) or isinstance(qsos, list):
            if isinstance(qsos, list) and len(qsos) > 100000:
                return jsonify({"ok": False, "error": "too many qsos"}), 413
            if isinstance(qso_summary, dict):
                def string_set(name):
                    values = qso_summary.get(name, [])
                    if not isinstance(values, list) or len(values) > 100000:
                        raise ValueError(name)
                    return {str(value)[:128] for value in values if str(value).strip()}
                try:
                    confirmed_calls = string_set("confirmedCalls")
                    confirmed_dxcc = string_set("confirmedDxcc")
                    worked_calls = string_set("workedCalls")
                    worked_dxcc = string_set("workedDxcc")
                    raw_bands = qso_summary.get("dxccByBand", {})
                    if not isinstance(raw_bands, dict) or len(raw_bands) > 100:
                        raise ValueError("dxccByBand")
                    dxcc_by_band = {}
                    for band, values in raw_bands.items():
                        if not isinstance(values, list) or len(values) > 10000:
                            raise ValueError("dxccByBand")
                        dxcc_by_band[str(band)[:16]] = {str(value)[:128] for value in values if str(value).strip()}
                    nexus_total = max(0, min(5_000_000, int(qso_summary.get("totalQso", 0))))
                    nexus_confirmed = max(0, min(nexus_total, int(qso_summary.get("totalConfirmed", 0))))
                except (TypeError, ValueError):
                    return jsonify({"ok": False, "error": "invalid qso summary"}), 400
            else:
                confirmed_calls = set()
                confirmed_dxcc = set()
                worked_calls = set()
                worked_dxcc = set()
                dxcc_by_band = {}
                for qso in qsos:
                    if not isinstance(qso, dict):
                        continue
                    call = str(qso.get("call") or "").upper()
                    entity = str(qso.get("entity") or "")
                    band = str(qso.get("band") or "")
                    confirmed = bool(qso.get("confirmed"))
                    if call:
                        worked_calls.add(call)
                    if entity:
                        worked_dxcc.add(entity)
                    if confirmed:
                        if call:
                            confirmed_calls.add(call)
                        if entity:
                            confirmed_dxcc.add(entity)
                            dxcc_by_band.setdefault(band, set()).add(entity)
                nexus_total = len(qsos)
                nexus_confirmed = sum(1 for qso in qsos if isinstance(qso, dict) and qso.get("confirmed"))
            with state.lotw_lock:
                current = {
                    "confirmed_calls": set(state.lotw_data.get("confirmed_calls", set())),
                    "confirmed_dxcc": set(state.lotw_data.get("confirmed_dxcc", set())),
                    "worked_calls": set(state.lotw_data.get("worked_calls", set())),
                    "worked_dxcc": set(state.lotw_data.get("worked_dxcc", set())),
                    "dxcc_by_band": {name: set(values) for name, values in state.lotw_data.get("dxcc_by_band", {}).items()},
                    "total_qso": int(state.lotw_data.get("total_qso", 0)),
                    "total_confirmed": int(state.lotw_data.get("total_confirmed", 0)),
                }
                # An upstream LoTW refresh replaces lotw_data wholesale. Preserve that independent
                # source layer whenever it differs from the exact composite we last published;
                # subtraction cannot do this because an entity may genuinely exist in both sources.
                nonlocal last_merged_qsos
                if last_merged_qsos is None or current != last_merged_qsos:
                    upstream_qsos.update(current)
                merged_bands = {name: set(values) for name, values in upstream_qsos["dxcc_by_band"].items()}
                for name, values in dxcc_by_band.items():
                    merged_bands.setdefault(name, set()).update(values)
                merged = {
                    "confirmed_calls": upstream_qsos["confirmed_calls"] | confirmed_calls,
                    "confirmed_dxcc": upstream_qsos["confirmed_dxcc"] | confirmed_dxcc,
                    "worked_calls": upstream_qsos["worked_calls"] | worked_calls,
                    "worked_dxcc": upstream_qsos["worked_dxcc"] | worked_dxcc,
                    "dxcc_by_band": merged_bands,
                    "total_qso": upstream_qsos["total_qso"] + nexus_total,
                    "total_confirmed": upstream_qsos["total_confirmed"] + nexus_confirmed,
                }
                state.lotw_data.update(merged)
                last_merged_qsos = {
                    **merged,
                    "confirmed_calls": set(merged["confirmed_calls"]),
                    "confirmed_dxcc": set(merged["confirmed_dxcc"]),
                    "worked_calls": set(merged["worked_calls"]),
                    "worked_dxcc": set(merged["worked_dxcc"]),
                    "dxcc_by_band": {name: set(values) for name, values in merged_bands.items()},
                }
                state.lotw_session["logged_in"] = bool(qsos) or state.lotw_session.get("logged_in", False)
                state.lotw_session["last_sync"] = time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime())

        synced_qsos = nexus_total if isinstance(qso_summary, dict) or isinstance(qsos, list) else 0
        return jsonify({"ok": True, "spots": len(state.spots_buffer), "qsos": synced_qsos})

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

    def embedded_meta_run():
        token = request.headers.get("X-META-TOKEN", "")
        remote = request.remote_addr or ""
        if state.META_RUN_TOKEN:
            if not hmac.compare_digest(token, state.META_RUN_TOKEN):
                return jsonify({"status": "forbidden"}), 403
        elif not (remote.startswith("192.168.") or remote.startswith("10.") or remote.startswith("127.")):
            return jsonify({"status": "forbidden"}), 403
        try:
            outdir = Path(state.META_DIR)
            outdir.mkdir(parents=True, exist_ok=True)
            log_path = Path(request.args.get("log", str(state.LOG_PATH_DEFAULT)))
            summary = build_summary(parse_log(log_path))
            (outdir / "summary.json").write_text(
                json.dumps(summary, ensure_ascii=False, indent=2), encoding="utf-8")
            return jsonify({"status": "ok", "spots": summary["spots"]})
        except Exception as error:
            return jsonify({"status": "error", "msg": str(error)}), 500

    app.view_functions["run_meta"] = embedded_meta_run
