import os
import tempfile
import threading
import unittest
from collections import deque
from pathlib import Path

from flask import Flask, request

from nexus_bridge import install_nexus_bridge


class _Predictor:
    def __init__(self):
        self.recorded = []

    def record_spot(self, spot, **_kwargs):
        self.recorded.append(spot)
        return None


class _State:
    def __init__(self):
        self.upstream_cluster_calls = []
        self.cluster_send_line = lambda line: self.upstream_cluster_calls.append(line) or False
        self.spots_buffer = deque(maxlen=200)
        self.predictor = _Predictor()
        self.watchlist = set()
        self._ANALYTICS_OK = False
        self.lotw_lock = threading.Lock()
        self.lotw_session = {"logged_in": False, "last_sync": "provider-time"}
        self.lotw_data = {
            "confirmed_calls": {"K1ABC", "UPONLY"},
            "confirmed_dxcc": {"Shared", "Upstream"},
            "worked_dxcc": {"Shared", "Upstream"},
            "dxcc_by_band": {"20m": {"Shared", "Upstream"}},
            "total_qso": 2,
            "total_confirmed": 2,
        }
        self.MY_CALL = ""
        self.user_qra = ""
        self.META_RUN_TOKEN = ""
        self.META_DIR = Path(tempfile.mkdtemp()) / "meta"
        self.LOG_PATH_DEFAULT = self.META_DIR.parent / "radio.log"
        self.LOG_PATH_DEFAULT.write_text("", encoding="utf-8")

    def _update_location_from_user_qra(self):
        return None


class NexusBridgeTests(unittest.TestCase):
    def setUp(self):
        self.previous_token = os.environ.get("NEURAL_DX_NEXUS_TOKEN")
        self.previous_embed = os.environ.get("NEURAL_DX_NEXUS_EMBED")
        os.environ["NEURAL_DX_NEXUS_TOKEN"] = "test-secret"
        os.environ["NEURAL_DX_NEXUS_EMBED"] = "1"
        self.temp = tempfile.TemporaryDirectory()
        self.previous_cwd = os.getcwd()
        os.chdir(self.temp.name)
        self.app = Flask(__name__)
        self.app.add_url_rule("/api/meta/run", "run_meta", lambda: ({"upstream": True}, 200), methods=["POST"])
        self.state = _State()
        install_nexus_bridge(self.app, self.state)
        def knowledge_consumer():
            if (not self.state.lotw_session["logged_in"]
                    and not self.state._nexus_knowledge_available()):
                return {"logged_in": False}
            call = (request.args.get("call") or "").upper()
            return {"logged_in": self.state.lotw_session["logged_in"],
                    "call_confirmed": call in self.state.lotw_data["confirmed_calls"]}
        self.app.add_url_rule("/test/knowledge-consumer", "knowledge_consumer", knowledge_consumer)
        self.client = self.app.test_client()
        self.headers = {"X-Nexus-Token": "test-secret"}

    def tearDown(self):
        os.chdir(self.previous_cwd)
        self.temp.cleanup()
        if self.previous_token is None:
            os.environ.pop("NEURAL_DX_NEXUS_TOKEN", None)
        else:
            os.environ["NEURAL_DX_NEXUS_TOKEN"] = self.previous_token
        if self.previous_embed is None:
            os.environ.pop("NEURAL_DX_NEXUS_EMBED", None)
        else:
            os.environ["NEURAL_DX_NEXUS_EMBED"] = self.previous_embed

    def sync(self, qsos):
        return self.client.post("/api/nexus/sync", headers=self.headers, json={"qsos": qsos})

    def test_auth_overlap_deletion_refresh_and_idempotence(self):
        self.assertEqual(self.client.get("/api/nexus/status").status_code, 401)
        first = [
            {"call": "K1ABC", "entity": "Shared", "band": "20m", "confirmed": True},
            {"call": "N0NEW", "entity": "Nexus", "band": "40m", "confirmed": True},
        ]
        self.assertEqual(self.sync(first).status_code, 200)
        self.assertEqual(self.sync(first).status_code, 200)
        self.assertEqual(self.state.lotw_data["confirmed_calls"], {"K1ABC", "UPONLY", "N0NEW"})

        # Removing an overlapping Nexus record must not erase the genuine upstream copy.
        self.assertEqual(self.sync(first[1:]).status_code, 200)
        self.assertIn("K1ABC", self.state.lotw_data["confirmed_calls"])
        self.assertEqual(self.state.lotw_data["dxcc_by_band"]["20m"], {"Shared", "Upstream"})

        # A later upstream collector refresh replaces the independent upstream layer.
        self.state.lotw_data = {
            "confirmed_calls": {"REFRESH"},
            "confirmed_dxcc": {"Refreshed"},
            "worked_dxcc": {"Refreshed"},
            "dxcc_by_band": {"15m": {"Refreshed"}},
            "total_qso": 1,
            "total_confirmed": 1,
        }
        self.assertEqual(self.sync(first[1:]).status_code, 200)
        self.assertEqual(self.state.lotw_data["confirmed_calls"], {"REFRESH", "N0NEW"})
        self.assertEqual(self.state.lotw_data["total_qso"], 1)
        self.assertEqual(self.state.lotw_session, {"logged_in": False, "last_sync": "provider-time"})
        self.assertEqual(self.state.nexus_qso_provenance["total_qso"], 1)

        summary = {
            "confirmedCalls": ["K2SUM"], "confirmedDxcc": ["Summary"],
            "workedCalls": ["K2SUM"], "workedDxcc": ["Summary"],
            "dxccByBand": {"10m": ["Summary"]}, "totalQso": 25, "totalConfirmed": 1,
        }
        response = self.client.post(
            "/api/nexus/sync", headers=self.headers, json={"qsoSummary": summary}
        )
        self.assertEqual(response.status_code, 200)
        self.assertEqual(response.get_json()["qsos"], 25)
        self.assertIn("K2SUM", self.state.lotw_data["confirmed_calls"])
        self.assertEqual(self.state.lotw_data["total_qso"], 1)
        self.assertEqual(self.state.nexus_qso_provenance["total_qso"], 25)
        self.assertFalse(self.state.lotw_session["logged_in"])
        consumer = self.client.get("/test/knowledge-consumer?call=K2SUM").get_json()
        self.assertTrue(consumer["call_confirmed"], "Nexus truth reaches a real consumer")

        # Restarting only the supervised child retains the provider baseline in the upstream
        # process state, so deleting the Nexus-only QSO cannot promote it into provider truth.
        restarted_app = Flask("same-state-child-restart")
        install_nexus_bridge(restarted_app, self.state)
        restarted = restarted_app.test_client()
        deleted = restarted.post("/api/nexus/sync", headers=self.headers, json={"qsoSummary": {
            "confirmedCalls": [], "confirmedDxcc": [], "workedCalls": [], "workedDxcc": [],
            "dxccByBand": {}, "totalQso": 0, "totalConfirmed": 0,
        }})
        self.assertEqual(deleted.status_code, 200)
        self.assertNotIn("K2SUM", self.state.lotw_data["confirmed_calls"])
        self.assertEqual(self.state.lotw_data["confirmed_calls"], {"REFRESH"})

        # A fresh embedded process starts from provider-only state. Nexus deletions do not
        # resurrect a prior process's composite state from disk.
        fresh_app = Flask("restart")
        fresh_state = _State()
        fresh_state.lotw_data["confirmed_calls"] = {"PROVIDER"}
        install_nexus_bridge(fresh_app, fresh_state)
        fresh = fresh_app.test_client()
        empty = fresh.post("/api/nexus/sync", headers=self.headers, json={"qsoSummary": {
            "confirmedCalls": [], "confirmedDxcc": [], "workedCalls": [], "workedDxcc": [],
            "dxccByBand": {}, "totalQso": 0, "totalConfirmed": 0,
        }})
        self.assertEqual(empty.status_code, 200)
        self.assertEqual(fresh_state.lotw_data["confirmed_calls"], {"PROVIDER"})

    def test_actions_are_validated_deduplicated_and_reject_when_full(self):
        self.assertFalse(self.state.cluster_send_line("DX nope BAD! control"))
        self.assertFalse(self.state.cluster_send_line("DX 9999999 VK9DX out-of-range"))
        self.assertTrue(self.state.cluster_send_line("DX 14195 VK9DX UP 5"))
        self.assertTrue(self.state.cluster_send_line("DX 14195 VK9DX UP 5"))
        actions = self.client.get("/api/nexus/actions", headers=self.headers).get_json()["actions"]
        self.assertEqual(len(actions), 1)
        self.assertEqual({key: actions[0][key] for key in ("type", "freqMhz", "call", "comment")},
                         {"type": "postSpot", "freqMhz": 14.195, "call": "VK9DX", "comment": "UP 5"})
        self.assertRegex(actions[0]["id"], r"^[0-9a-f]{32}$")
        self.assertEqual(self.state.upstream_cluster_calls, [])
        replay = self.client.get("/api/nexus/actions", headers=self.headers).get_json()["actions"]
        self.assertEqual(replay, actions)
        self.assertEqual(self.client.post("/api/nexus/actions/ack", headers=self.headers,
                                         json={"ids": [actions[0]["id"]]}).status_code, 200)
        self.assertEqual(self.client.get("/api/nexus/actions", headers=self.headers).get_json()["actions"], [])
        for index in range(100):
            self.assertTrue(self.state.cluster_send_line(f"DX 14195 K{index:03d}DX item-{index}"))
        self.assertFalse(self.state.cluster_send_line("DX 14195 K999DX rejected-at-capacity"))
        bounded = self.client.get("/api/nexus/actions", headers=self.headers).get_json()["actions"]
        self.assertEqual(len(bounded), 100)
        self.assertEqual(bounded[0]["comment"], "item-0")
        self.assertEqual(bounded[-1]["comment"], "item-99")
        oversized = self.client.post(
            "/api/nexus/sync", headers=self.headers, json={"spots": [{}] * 5001}
        )
        self.assertEqual(oversized.status_code, 413)

    def test_pending_action_survives_child_restart_until_acknowledged(self):
        self.assertTrue(self.state.cluster_send_line("DX 14195 VK9DX UP 5"))
        pending = self.client.get("/api/nexus/actions", headers=self.headers).get_json()["actions"]
        restarted_app = Flask("child-restart")
        restarted_state = _State()
        install_nexus_bridge(restarted_app, restarted_state)
        replay = restarted_app.test_client().get(
            "/api/nexus/actions", headers=self.headers).get_json()["actions"]
        self.assertEqual(replay, pending)

    def test_full_pending_queue_survives_restart_without_loss(self):
        for index in range(100):
            self.assertTrue(self.state.cluster_send_line(f"DX 14195 K{index:03d}DX item-{index}"))
        before = self.client.get("/api/nexus/actions", headers=self.headers).get_json()["actions"]
        restarted_app = Flask("full-queue-restart")
        restarted_state = _State()
        install_nexus_bridge(restarted_app, restarted_state)
        after = restarted_app.test_client().get(
            "/api/nexus/actions", headers=self.headers).get_json()["actions"]
        self.assertEqual(after, before)
        self.assertFalse(restarted_state.cluster_send_line("DX 14195 K999DX still-full"))

    def test_spot_identity_keeps_distinct_same_minute_and_deduplicates_resync(self):
        first = {"dx_call": "VK9DX", "de_call": "K1AAA", "freq": 14195.0,
                 "timestamp": 1_700_000_001.1, "band": "20m", "mode": "SSB", "comment": "UP 5"}
        second = {**first, "de_call": "K2BBB", "freq": 14196.0}
        response = self.client.post("/api/nexus/sync", headers=self.headers,
                                    json={"spots": [first, second]})
        self.assertEqual(response.status_code, 200)
        self.assertEqual(len(self.state.predictor.recorded), 2)
        self.client.post("/api/nexus/sync", headers=self.headers, json={"spots": [first, second]})
        self.assertEqual(len(self.state.predictor.recorded), 2)

    def test_spot_identity_lru_is_bounded_and_evicts_the_oldest(self):
        for page in range(100):
            spots = [{"spot_id": f"spot-{page * 200 + index}", "dx_call": "VK9DX"}
                     for index in range(200)]
            self.assertEqual(self.client.post("/api/nexus/sync", headers=self.headers,
                                              json={"spots": spots}).status_code, 200)
        self.assertEqual(len(self.state.predictor.recorded), 20_000)
        newest = {"spot_id": "spot-new", "dx_call": "VK9DX"}
        self.client.post("/api/nexus/sync", headers=self.headers, json={"spots": [newest]})
        oldest = {"spot_id": "spot-0", "dx_call": "VK9DX"}
        self.client.post("/api/nexus/sync", headers=self.headers, json={"spots": [oldest]})
        self.assertEqual(len(self.state.predictor.recorded), 20_002)

    def test_embedded_meta_requires_token_and_rejects_custom_paths(self):
        self.assertEqual(self.client.post("/api/meta/run").status_code, 403)
        self.assertEqual(self.client.post("/api/meta/run", headers={"X-Nexus-Token": "wrong"}).status_code, 403)
        self.assertEqual(self.client.post("/api/meta/run?log=/etc/passwd", headers=self.headers).status_code, 400)
        response = self.client.post("/api/meta/run", headers=self.headers)
        self.assertEqual(response.status_code, 200)
        self.assertEqual(response.get_json()["status"], "ok")


if __name__ == "__main__":
    unittest.main()
