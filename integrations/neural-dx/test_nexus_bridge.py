import os
import threading
import unittest
from collections import deque

from flask import Flask

from nexus_bridge import install_nexus_bridge


class _Predictor:
    def record_spot(self, *_args, **_kwargs):
        return None


class _State:
    def __init__(self):
        self.cluster_send_line = lambda _line: False
        self.spots_buffer = deque(maxlen=200)
        self.predictor = _Predictor()
        self.watchlist = set()
        self._ANALYTICS_OK = False
        self.lotw_lock = threading.Lock()
        self.lotw_session = {}
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

    def _update_location_from_user_qra(self):
        return None


class NexusBridgeTests(unittest.TestCase):
    def setUp(self):
        self.previous_token = os.environ.get("NEURAL_DX_NEXUS_TOKEN")
        os.environ["NEURAL_DX_NEXUS_TOKEN"] = "test-secret"
        self.app = Flask(__name__)
        self.state = _State()
        install_nexus_bridge(self.app, self.state)
        self.client = self.app.test_client()
        self.headers = {"X-Nexus-Token": "test-secret"}

    def tearDown(self):
        if self.previous_token is None:
            os.environ.pop("NEURAL_DX_NEXUS_TOKEN", None)
        else:
            os.environ["NEURAL_DX_NEXUS_TOKEN"] = self.previous_token

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
        self.assertEqual(self.state.lotw_data["total_qso"], 2)

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

    def test_actions_are_validated_deduplicated_and_bounded(self):
        self.assertFalse(self.state.cluster_send_line("DX nope BAD! control"))
        self.assertFalse(self.state.cluster_send_line("DX 9999999 VK9DX out-of-range"))
        self.assertTrue(self.state.cluster_send_line("DX 14195 VK9DX UP 5"))
        self.assertTrue(self.state.cluster_send_line("DX 14195 VK9DX UP 5"))
        actions = self.client.get("/api/nexus/actions", headers=self.headers).get_json()["actions"]
        self.assertEqual(actions, [{"type": "postSpot", "freqMhz": 14.195, "call": "VK9DX", "comment": "UP 5"}])
        oversized = self.client.post(
            "/api/nexus/sync", headers=self.headers, json={"spots": [{}] * 5001}
        )
        self.assertEqual(oversized.status_code, 413)


if __name__ == "__main__":
    unittest.main()
