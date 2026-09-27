"""Narrow localhost bridge from RD-Nexus into the pinned Neural DX runtime.

This file is copied into the temporary packaging tree. The upstream submodule is
never edited, which keeps a future fast-forward and audit mechanically clear.
"""

import hmac
import json
import os
import re
import secrets
import threading
import time
from collections import OrderedDict, deque
from pathlib import Path
from flask import jsonify, request
from tools.log_meta_analyzer import build_summary, parse_log


def install_nexus_bridge(app, state):
    app.config["MAX_CONTENT_LENGTH"] = 16 * 1024 * 1024
    expected = os.environ.get("NEURAL_DX_NEXUS_TOKEN", "")
    embedded = os.environ.get("NEURAL_DX_NEXUS_EMBED") == "1"
    seen = OrderedDict()
    actions = OrderedDict()
    actions_lock = threading.Lock()
    recent_actions = {}
    action_journal_error = None
    upstream_qsos = {
        "confirmed_calls": set(),
        "confirmed_dxcc": set(),
        "worked_calls": set(),
        "worked_dxcc": set(),
        "dxcc_by_band": {},
        "total_qso": 0,
        "total_confirmed": 0,
    }
    def copy_qso_layer(layer):
        return {
            "confirmed_calls": set(layer.get("confirmed_calls", set())),
            "confirmed_dxcc": set(layer.get("confirmed_dxcc", set())),
            "worked_calls": set(layer.get("worked_calls", set())),
            "worked_dxcc": set(layer.get("worked_dxcc", set())),
            "dxcc_by_band": {name: set(values) for name, values in layer.get("dxcc_by_band", {}).items()},
            "total_qso": int(layer.get("total_qso", 0)),
            "total_confirmed": int(layer.get("total_confirmed", 0)),
        }

    saved_upstream = getattr(state, "_nexus_upstream_qsos", None)
    if isinstance(saved_upstream, dict):
        upstream_qsos = copy_qso_layer(saved_upstream)
    saved_composite = getattr(state, "_nexus_last_merged_qsos", None)
    last_merged_qsos = copy_qso_layer(saved_composite) if isinstance(saved_composite, dict) else None
    upstream_cluster_send_line = state.cluster_send_line
    upstream_meta_run = app.view_functions.get("run_meta")
    upstream_spot_capacity = getattr(state, "_nexus_spot_capacity", state.spots_buffer.maxlen)
    state._nexus_spot_capacity = upstream_spot_capacity
    state.spots_buffer = deque(state.spots_buffer, maxlen=upstream_spot_capacity * 2)
    actions_path = Path("data/nexus_actions.json")

    def persist_actions():
        if not embedded:
            return
        actions_path.parent.mkdir(parents=True, exist_ok=True)
        pending = actions_path.with_suffix(".json.tmp")
        with pending.open("w", encoding="utf-8") as handle:
            json.dump(list(actions.values()), handle, separators=(",", ":"))
            handle.flush()
            os.fsync(handle.fileno())
        os.replace(pending, actions_path)

    if embedded and actions_path.is_file():
        try:
            restored = json.loads(actions_path.read_text(encoding="utf-8"))
            for action in restored[-100:] if isinstance(restored, list) else []:
                action_id = action.get("id") if isinstance(action, dict) else None
                if isinstance(action_id, str) and re.fullmatch(r"[0-9a-f]{32}", action_id):
                    actions[action_id] = action
                    signature = (action.get("freqMhz"), action.get("call"), action.get("comment"))
                    recent_actions[signature] = time.monotonic()
        except (OSError, ValueError, TypeError) as error:
            # Fail closed and expose the recovery requirement; never silently discard intent.
            actions.clear()
            recent_actions.clear()
            action_journal_error = f"pending action journal requires recovery: {error}"

    def authorised():
        supplied = request.headers.get("X-Nexus-Token", "")
        return bool(expected) and hmac.compare_digest(expected, supplied)

    def nexus_cluster_line(line):
        """In an embedded runtime, queue validated intent for Nexus, the sole TX authority."""
        if embedded and action_journal_error:
            return False
        if not embedded and upstream_cluster_send_line(line):
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
            action_id = secrets.token_hex(16)
            actions[action_id] = {
                "id": action_id,
                "type": "postSpot",
                "freqMhz": freq_mhz,
                "call": call,
                "comment": comment,
            }
            while len(actions) > 100:
                actions.popitem(last=False)
            persist_actions()
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
            now = time.monotonic()
            while seen:
                _oldest, recorded = next(iter(seen.items()))
                if now - recorded <= 6 * 60 * 60:
                    break
                seen.popitem(last=False)
            for spot in incoming[-upstream_spot_capacity:]:
                if not isinstance(spot, dict) or not spot.get("dx_call"):
                    continue
                spot = dict(spot)
                spot["nexus_source"] = "shared"
                state.spots_buffer.append(spot)
                supplied_id = spot.get("spot_id") or spot.get("id")
                identity = ("id", str(supplied_id)) if supplied_id else (
                    "full",
                    str(spot.get("dx_call") or "").upper(),
                    str(spot.get("spotter") or spot.get("source") or "").upper(),
                    str(spot.get("freq") or spot.get("freq_mhz") or ""),
                    str(spot.get("timestamp") or ""),
                    str(spot.get("band") or ""),
                    str(spot.get("mode") or ""),
                    str(spot.get("comment") or ""),
                )
                if identity not in seen:
                    seen[identity] = now
                    try:
                        state.predictor.record_spot(spot, is_watchlist=spot.get("dx_call", "").upper() in state.watchlist)
                    except Exception:
                        pass
                    if state._ANALYTICS_OK:
                        try:
                            state._analytics_mod.record_spot(spot)
                        except Exception:
                            pass
                else:
                    seen.move_to_end(identity)
                    seen[identity] = now
            while len(seen) > 20000:
                seen.popitem(last=False)

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
                    state._nexus_upstream_qsos = copy_qso_layer(upstream_qsos)
                merged_bands = {name: set(values) for name, values in upstream_qsos["dxcc_by_band"].items()}
                for name, values in dxcc_by_band.items():
                    merged_bands.setdefault(name, set()).update(values)
                merged = {
                    "confirmed_calls": upstream_qsos["confirmed_calls"] | confirmed_calls,
                    "confirmed_dxcc": upstream_qsos["confirmed_dxcc"] | confirmed_dxcc,
                    "worked_calls": upstream_qsos["worked_calls"] | worked_calls,
                    "worked_dxcc": upstream_qsos["worked_dxcc"] | worked_dxcc,
                    "dxcc_by_band": merged_bands,
                    # These counters remain the provider's counters. Summing sources is wrong when
                    # the same QSO is present in both Nexus and LoTW.
                    "total_qso": upstream_qsos["total_qso"],
                    "total_confirmed": upstream_qsos["total_confirmed"],
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
                state._nexus_last_merged_qsos = copy_qso_layer(last_merged_qsos)
                state.nexus_qso_provenance = {
                    "total_qso": nexus_total,
                    "total_confirmed": nexus_confirmed,
                    "last_sync": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()),
                }

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
        if action_journal_error:
            return jsonify({"ok": False, "error": action_journal_error}), 500
        with actions_lock:
            pending = list(actions.values())
        return jsonify({"ok": True, "actions": pending})

    @app.post("/api/nexus/actions/ack")
    def nexus_actions_ack():
        if not authorised():
            return jsonify({"ok": False, "error": "unauthorised"}), 401
        payload = request.get_json(silent=True) or {}
        ids = payload.get("ids") if isinstance(payload, dict) else None
        if not isinstance(ids, list) or len(ids) > 100 or any(
                not isinstance(action_id, str) or not re.fullmatch(r"[0-9a-f]{32}", action_id)
                for action_id in ids):
            return jsonify({"ok": False, "error": "invalid action ids"}), 400
        with actions_lock:
            for action_id in ids:
                actions.pop(action_id, None)
            persist_actions()
        return jsonify({"ok": True})

    def embedded_meta_run():
        if not embedded:
            return upstream_meta_run()
        meta_token = request.headers.get("X-META-TOKEN", "")
        meta_authorised = bool(state.META_RUN_TOKEN) and hmac.compare_digest(
            meta_token, state.META_RUN_TOKEN)
        if not (authorised() or meta_authorised):
            return jsonify({"status": "forbidden"}), 403
        if "log" in request.args:
            return jsonify({"status": "error", "msg": "custom log paths are disabled"}), 400
        try:
            outdir = Path(state.META_DIR)
            outdir.mkdir(parents=True, exist_ok=True)
            log_path = Path(state.LOG_PATH_DEFAULT)
            summary = build_summary(parse_log(log_path))
            (outdir / "summary.json").write_text(
                json.dumps(summary, ensure_ascii=False, indent=2), encoding="utf-8")
            return jsonify({"status": "ok", "spots": summary["spots"]})
        except Exception as error:
            return jsonify({"status": "error", "msg": str(error)}), 500

    if upstream_meta_run is not None:
        app.view_functions["run_meta"] = embedded_meta_run
