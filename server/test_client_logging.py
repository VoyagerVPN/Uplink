import http.client
import io
import json
import re
import threading
import tempfile
import unittest
from contextlib import redirect_stderr
from unittest.mock import patch
from pathlib import Path

import client_api
from client_api import ClientRequest, ClientServer
from instance import ApiError


class ClientLoggingTest(unittest.TestCase):
    def test_maintenance_allows_version_probe_without_serving_membership_changes(self):
        with tempfile.TemporaryDirectory() as directory:
            flag = Path(directory) / "updating"
            flag.touch()
            with patch.object(client_api, "MAINTENANCE", flag), patch.object(client_api, "networks") as state:
                self.assertEqual(client_api.route("POST", "/v1/ping", {}), (200, {"server_revision": 3}))
                with self.assertRaises(ApiError) as fault:
                    client_api.route("POST", "/v1/redeem", {})
                self.assertEqual(fault.exception.status, 503)
                state.assert_not_called()

    def test_join_stages_are_distinguishable_without_logging_credentials(self):
        ticket = "ac" * 32

        def route(method, path, data):
            if path == "/v1/login":
                return 200, {"token": ticket}
            if path == "/v1/redeem":
                raise ApiError(403, "Membership revoked")
            raise ApiError(403, "private-detail")

        log = io.StringIO()
        with ClientServer(("127.0.0.1", 0), ClientRequest) as server, redirect_stderr(log), patch.object(ClientRequest, "route", staticmethod(route)):
            thread = threading.Thread(target=server.serve_forever)
            thread.start()
            try:
                conn = http.client.HTTPConnection(*server.server_address)
                for path in ("/v1/login", "/v1/redeem", "/private-detail?secret"):
                    conn.request("POST", path, json.dumps({"password": "my-secret", "token": ticket}))
                    conn.getresponse().read()
                conn.request("SECRET-METHOD", "/secret")
                conn.getresponse().read()
                conn.close()
            finally:
                server.shutdown()
                thread.join()
        lines = log.getvalue()
        for secret in (ticket, "my-secret", "private-detail", "SECRET-METHOD", "/secret"):
            self.assertNotIn(secret, lines)
        self.assertIn("stage=login status=200 reason=ok", lines)
        self.assertIn("stage=redeem status=403 reason=membership_revoked", lines)
        self.assertIn("stage=unknown status=403", lines)
        tags = re.findall(r"ticket=([0-9a-f]{16})", lines)
        self.assertEqual(len(tags), 2)
        self.assertEqual(tags[0], tags[1])
