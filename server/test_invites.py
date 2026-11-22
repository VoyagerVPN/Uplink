import tempfile
import unittest
from pathlib import Path
from unittest.mock import Mock

from instance import ApiError
from invites import Invitations


class InvitationsTest(unittest.TestCase):
    def setUp(self):
        self.directory = tempfile.TemporaryDirectory()
        self.addCleanup(self.directory.cleanup)
        self.network = "0123456789000001"
        self.node = "1234567890"
        self.session = "a" * 64
        self.members = {}
        def controller(path, payload=None):
            if path=="/controller/network": return [self.network]
            if path.endswith("/member"):
                return dict.fromkeys(self.members, 1)
            if "/member/" in path:
                member = self.members[path[-10:]]
                member.update(payload or {})
                return member.copy()
            return {"id": self.network, "name": "LAN", "private": True}
        self.controller = Mock(side_effect=controller)
        self.api = Invitations(Path(self.directory.name) / "state.db", self.controller, lambda: 100, verify_device=lambda *args: None)

    def pending(self):
        self.members[self.node] = {"authorized": False, "ipAssignments": ["10.9.0.23"]}

    def test_pending_retry_and_replay(self):
        token = self.api.issue(self.network)
        self.assertEqual(self.api.lookup(token)["id"], self.network)
        with self.assertRaises(ApiError) as pending:
            self.api.redeem(token, self.node, self.session, "Mira")
        self.assertEqual(pending.exception.status, 409)
        self.pending()
        self.api.redeem(token, self.node, self.session, "Mira")
        self.api.redeem(token, self.node, self.session, "Mira")
        self.assertTrue(self.members[self.node]["authorized"])
        for node, session in [(self.node, "b" * 64), ("9999999999", self.session)]:
            with self.assertRaises(ApiError):
                self.api.redeem(token, node, session, "Other")
        result = self.api.sync(self.session, "Orbit")
        self.assertEqual(result["members"][0]["nick"], "Orbit")
        self.assertEqual(result["members"][0]["ip"], "10.9.0.23")
        self.members[self.node]["authorized"] = False
        with self.assertRaises(ApiError):
            self.api.redeem(token, self.node, self.session, "Mira")
        with self.assertRaises(ApiError):
            self.api.sync(self.session, "Mira")

    def test_expiry_and_existing_member(self):
        self.pending()
        token = self.api.issue(self.network)
        self.members[self.node]["authorized"] = True
        with self.assertRaises(ApiError):
            self.api.redeem(token, self.node, self.session, "Fake")
        self.api.now = lambda: 100000
        with self.assertRaises(ApiError):
            self.api.lookup(token)

    def test_roles_rename_transfer_and_instance_boundary(self):
        from client_api import route
        self.pending()
        self.api.redeem(self.api.issue(self.network), self.node, self.session, "First")
        second = "2345678901"
        other = "b"*64
        self.members[second] = {"authorized": False, "ipAssignments": ["10.9.0.24"]}
        self.api.redeem(self.api.issue(self.network), second, other, "Second")
        with self.assertRaises(ApiError):
            self.api.manage(self.session, "name", "LAN 2")
        self.api.assign_owner(self.network, self.node)
        self.api.manage(self.session, "role", second)
        self.api.manage(other, "name", "LAN 2")
        self.api.manage(self.session, "transfer", second)
        with self.assertRaises(ApiError):
            self.api.manage(self.session, "name", "LAN 3")
        self.api.manage(other, "name", "New LAN")
        for session in (other, self.session):
            with self.assertRaises(ApiError) as denied:
                route("PUT", "/v1/networks/0123456789000002", {"session":session})
            self.assertEqual(denied.exception.status, 403)
