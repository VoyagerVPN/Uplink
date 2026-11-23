import tempfile
import unittest
from pathlib import Path
import ipaddress

from instance import ApiError
from invites import Invitations


class GroupTrafficTest(unittest.TestCase):
    def setUp(self):
        directory = tempfile.TemporaryDirectory()
        self.addCleanup(directory.cleanup)
        self.members = {}
        self.networks = {}
        self.transport = "0123456789000001"
        self.api = Invitations(Path(directory.name) / "members.sqlite3", self.controller, verify_device=lambda *args: None)
        self.a, self.b = "1" * 32, "2" * 32

    def controller(self, path, payload=None):
        if path == "/status":
            return {"address": self.transport[:10]}
        if path == "/controller/network":
            return list(self.networks)
        bits = path.split("/")
        if len(bits) == 5:
            return dict.fromkeys(self.members)
        if len(bits) == 6:
            item = self.members[bits[-1]]
        else:
            item = self.networks.setdefault(bits[-1], {"id": bits[-1]})
        item.update(payload or {})
        return item.copy()

    def setup_groups(self):
        for group, name in ((self.a, "A"), (self.b, "B")):
            self.api.create_group(group, {"name": name, "transport": self.transport, "subnet": "10.188.37.0/24"})

    def test_sync_distinguishes_a_missing_saved_group_from_a_denied_membership(self):
        self.setup_groups()
        for network, expected in [(self.a, 403), ("3" * 32, 410), ("f" * 16, 410), (self.transport, 403), ("bad", 403)]:
            with self.subTest(network=network), self.assertRaises(ApiError) as denied:
                self.api.sync("f" * 64, "Former member", network=network)
            self.assertEqual(denied.exception.status, expected)

    def test_owner_can_create_networks_by_name_on_a_new_instance(self):
        self.networks[self.transport] = {
            "id": self.transport, "name": "Legacy", "routes": [{"target": "10.0.0.0/9"}]
        }
        first_status, first = self.api.create_group(self.a, {"name": "Friday"})
        self.assertEqual(first_status, 201)
        shared = first["transport"]
        self.assertNotEqual(shared, self.transport)
        self.assertTrue(shared.startswith(self.transport[:10]))
        subnet = ipaddress.IPv4Network(self.networks[shared]["routes"][0]["target"])
        self.assertEqual(subnet.prefixlen, 24)
        self.assertFalse(subnet.overlaps(ipaddress.IPv4Network("10.0.0.0/9")))
        self.assertEqual(self.api.create_group(self.a, {"name": "Friday"}), (200, first))
        self.assertEqual(self.api.create_group(self.b, {"name": "Weekend"})[1]["transport"], shared)
        with self.assertRaises(ApiError) as conflict:
            self.api.create_group("3" * 32, {"name": " friday "})
        self.assertEqual(conflict.exception.status, 409)

    def test_automatic_setup_can_retry_after_no_subnet_was_available(self):
        self.networks[self.transport] = {
            "name": "Existing", "routes": [{"target": "10.0.0.0/8"}]
        }
        with self.assertRaises(ApiError) as busy:
            self.api.create_group(self.a, {"name": "Friday"})
        self.assertEqual(busy.exception.status, 409)
        self.assertEqual(list(self.networks), [self.transport])
        status, _ = self.api.create_group(self.a, {"name": "Friday", "transport": self.transport[:10] + "ff8100",
                                                   "subnet": "172.18.0.0/24"})
        self.assertEqual(status, 201)

    def test_two_groups_one_transport_and_separate_rosters(self):
        self.setup_groups()
        node, peer = "b"*10, "a"*10
        for i, who in enumerate((node, peer)):
            self.members[who] = {"authorized": False, "tags": [], "ipAssignments": [f"10.188.37.{20+i}"]}
        first = self.api.issue(self.a)
        self.assertEqual(self.api.lookup(first)["transport"], self.transport)
        self.api.redeem(first, node, "1"*64, "Both")
        second = self.api.issue(self.b)
        with self.assertRaises(ApiError):
            self.api.redeem(second, node, "2"*64, "Wrong")
        self.api.redeem(second, node, "2"*64, "Both", "1"*64)
        self.api.redeem(self.api.issue(self.b), peer, "3"*64, "Only B")
        self.assertEqual([m["node"] for m in self.api.sync("1"*64, "Both")["members"]], [node])
        self.assertEqual(len(self.api.sync("2"*64, "Both")["members"]), 2)
        self.api.leave("2"*64)
        self.api.leave("2"*64)
        self.assertTrue(self.members[node]["authorized"])
        self.assertEqual(len(self.api.sync("3"*64, "Only B")["members"]), 1)
        self.assertEqual(len(self.api.sync("1"*64, "Both")["members"]), 1)
        self.api.leave("1"*64)
        self.assertFalse(self.members[node]["authorized"])


    def test_owner_operations_and_revocation_are_scoped_to_group(self):
        self.setup_groups()
        admin, peer = "c"*10, "d"*10
        for n in (admin, peer):
            self.members[n] = {"authorized":False, "tags":[], "ipAssignments":[]}
        self.api.redeem(self.api.issue(self.a),admin,"a"*64,"Owner")
        self.api.assign_owner(self.a,admin)
        self.api.redeem(self.api.issue(self.b),admin,"b"*64,"Owner","a"*64)
        self.api.redeem(self.api.issue(self.b),peer,"c"*64,"Peer")
        with self.assertRaises(ApiError):
            self.api.manage("a"*64,"remove",peer)
        self.api.redeem(self.api.issue(self.a),peer,"d"*64,"Peer","c"*64)
        self.api.manage("a"*64,"name","Renamed")
        self.assertEqual(self.api.sync("b"*64,"Owner")["name"],"B")
        self.api.manage("a"*64,"remove",peer)
        self.assertEqual(len(self.api.sync("c"*64,"Peer")["members"]),2)
        with self.assertRaises(ApiError):
            self.api.sync("d"*64,"Peer")
        self.api.leave("d"*64)
        with self.assertRaises(ApiError):
            self.api.issue(self.transport)

    def test_invitation_methods_restrict_sharing(self):
        self.setup_groups()
        who="e"*10
        self.members[who]={"authorized":False,"tags":[],"ipAssignments":[]}
        self.api.redeem(self.api.issue(self.a),who,"1"*64,"Host")
        self.api.assign_owner(self.a,who)
        self.api.manage("1"*64,"link","off")
        with self.assertRaises(ApiError):
            self.api.share("1"*64)
        self.api.manage("1"*64,"qr","on")
        token=self.api.share("1"*64)
        self.assertEqual(self.api.lookup(token)["id"],self.a)
        self.api.manage("1"*64,"qr","off")
        with self.assertRaises(ApiError):
            self.api.lookup(token)

    def test_group_names_are_unique_on_create_and_rename(self):
        self.setup_groups()
        with self.assertRaises(ApiError) as duplicate:
            self.api.create_group("3"*32, {"name":"  a  ", "transport":self.transport,"subnet":"10.188.37.0/24"})
        self.assertEqual(duplicate.exception.status,409)
        node="e"*10
        self.members[node]={"authorized":False,"tags":[],"ipAssignments":[]}
        self.api.redeem(self.api.issue(self.b),node,"2"*64,"Owner")
        self.api.assign_owner(self.b,node)
        with self.assertRaises(ApiError):
            self.api.manage("2"*64,"name","a")
        self.api.manage("2"*64,"name"," Кооп ")
        with self.assertRaises(ApiError):
            self.api.create_group("4"*32,{"name":"КООП","transport":self.transport,"subnet":"10.188.37.0/24"})

    def test_password_login_uses_current_name_rotation_and_existing_enrollment(self):
        self.setup_groups()
        node,friend="e"*10,"c"*10
        for who in (node, friend):
            self.members[who]={"authorized":False,"tags":[],"ipAssignments":[]}
        self.api.redeem(self.api.issue(self.a),node,"1"*64,"Owner")
        self.api.assign_owner(self.a,node)
        first = self.api.manage("1"*64,"password","secret one")["password_revision"]
        self.assertEqual(self.api.sync("1"*64,"Owner")["methods"]["password_revision"], first)
        token=self.api.login(" a ","secret one")
        self.assertEqual(self.api.lookup(token)["id"],self.a)
        with self.assertRaises(ApiError): self.api.login("B","secret one")
        second = self.api.manage("1"*64,"password","secret two")["password_revision"]
        self.assertNotEqual(first, second)
        self.assertEqual(self.api.sync("1"*64,"Owner")["methods"]["password_revision"], second)
        with self.assertRaises(ApiError): self.api.lookup(token)
        with self.assertRaises(ApiError): self.api.login("A","secret one")
        self.api.manage("1"*64,"name","Друзья")
        with self.assertRaises(ApiError): self.api.login("A","secret two")
        self.api.redeem(self.api.login("друзья","secret two"),friend,"2"*64,"Friend")
        with self.assertRaises(ApiError): self.api.manage("2"*64,"password","x")
        self.api.manage("1"*64,"password-mode","off")
        self.assertNotIn("password_revision", self.api.sync("1"*64,"Owner")["methods"])
        with self.assertRaises(ApiError): self.api.login("Друзья","secret two")
        self.assertEqual(len(self.api.sync("2"*64,"Friend")["members"]),2)

    def test_interrupted_enrollment_can_resume_only_with_same_device_session(self):
        from unittest.mock import patch
        self.setup_groups()
        node = "b" * 10
        self.members[node] = {"authorized": False, "tags": [], "ipAssignments": []}
        ticket = self.api.issue(self.a)
        original = self.api.directory
        def transient(db, path, payload=None):
            if payload == {"authorized": True}:
                raise ApiError(503, "Controller unavailable")
            return original(db, path, payload)
        with patch.object(self.api, "directory", side_effect=transient), self.assertRaises(ApiError):
            self.api.redeem(ticket, node, "1"*64, "Peer")
        second = self.api.issue(self.a)
        with self.assertRaises(ApiError):
            self.api.redeem(second, node, "2"*64, "Other")
        self.api.redeem(second, node, "1"*64, "Peer")
        self.assertEqual(self.api.sync("1"*64, "Peer")["id"], self.a)
        with self.assertRaises(ApiError):
            self.api.lookup(ticket)

    def test_network_name_length_and_printable_characters(self):
        self.setup_groups()
        node = "a"*10
        self.members[node] = {"authorized":False, "tags":[], "ipAssignments":[]}
        self.api.redeem(self.api.issue(self.a), node, "1"*64, "Owner")
        self.api.assign_owner(self.a, node)
        self.api.manage("1"*64, "name", "Я"*64)
        for bad in ("я"*65, "а\nб"):
            with self.assertRaises(ApiError):
                self.api.create_group("3"*32, {"name":bad, "transport":self.transport, "subnet":"10.188.37.0/24"})
            with self.assertRaises(ApiError):
                self.api.manage("1"*64, "name", bad)

    def test_failed_first_group_does_not_occupy_transport_slot(self):
        self.networks["012345678977ab01"] = {"name":"Taken", "routes":[], "id":"012345678977ab01"}
        with self.assertRaises(ApiError):
            self.api.create_group(self.a, {"name":"taken","transport":self.transport,"subnet":"10.188.37.0/24"})
        self.api.create_group(self.b, {"name":"Free","transport":"0123456789ffffdf","subnet":"10.188.38.0/24"})

    def test_password_http_login_and_budget(self):
        import http.client
        import json
        import threading
        from unittest.mock import patch
        import client_api
        self.setup_groups()
        owner, peer = "a"*10, "b"*10
        for node in (owner, peer):
            self.members[node] = {"authorized":False, "tags":[], "ipAssignments":[]}
        self.api.redeem(self.api.issue(self.a), owner, "1"*64, "Owner")
        self.api.assign_owner(self.a, owner)
        self.api.manage("1"*64, "password", "test pass")
        class Handler(client_api.ClientRequest):
            def log_request(self, *args): pass
        def post(endpoint, **data):
            conn = http.client.HTTPConnection(*server.server_address, timeout=4)
            try:
                conn.request("POST", "/v1/"+endpoint, json.dumps(data), {"Content-Type":"application/json"})
                reply = conn.getresponse()
                return reply.status, json.loads(reply.read())
            finally:
                conn.close()
        with patch("client_api.Invitations", return_value=self.api), client_api.ClientServer(("127.0.0.1",0), Handler) as server:
            with patch("client_api.LOGIN_BUDGET", client_api.LoginBudget(2, 60)):
                worker = threading.Thread(target=server.serve_forever, daemon=True)
                worker.start()
                try:
                    self.assertEqual(post("login", name="B", password="test pass")[0],403)
                    status, result = post("login", name="a", password="test pass")
                    self.assertEqual(status,200)
                    self.assertEqual(post("redeem", token=result["token"], node=peer, session="2"*64, nick="Friend")[0],200)
                    self.assertEqual(post("login", name="a", password="test pass")[0],429)
                    self.assertEqual(post("ping")[0],200)
                    import sqlite3
                    with patch.object(self.api,"sync",side_effect=sqlite3.OperationalError("database is locked")):
                        code, failure = post("sync", session="2"*64, nick="Friend")
                        self.assertEqual(code,503)
                        self.assertEqual(failure,{"error":"Local service unavailable"})
                    self.assertEqual(post("sync", session="2"*64, nick="Friend")[1]["id"],self.a)
                finally:
                    server.shutdown()
                    worker.join()

    def test_old_owner_requires_device_verification_before_recovering_privileges(self):
        from unittest.mock import Mock
        self.setup_groups()
        node, session = "d"*10, "3"*64
        self.members[node] = {"authorized":False,"tags":[],"ipAssignments":[],"identity":"controller identity"}
        self.api.redeem(self.api.issue(self.a), node, session, "Owner")
        self.api.assign_owner(self.a, node)
        with self.api.transaction() as db:
            db.execute("UPDATE invitations SET device_verified=0")
        self.assertTrue(self.api.sync(session,"Guest")["needs_proof"])
        self.assertEqual(self.api.sync(session,"Guest")["members"], [])
        for invoke in (lambda:self.api.manage(session,"name","Changed"),lambda:self.api.share(session),
                       lambda:self.api.leave(session),lambda:self.api.assign_owner(self.a,node)):
            with self.assertRaises(ApiError):
                invoke()
        self.api.verify_device = Mock(side_effect=ApiError(403,"Invalid device proof"))
        with self.assertRaises(ApiError):
            self.api.attest(session, {})
        self.assertTrue(self.api.sync(session,"Owner")["needs_proof"])
        self.api.verify_device = Mock()
        self.api.attest(session, {"proof":"fixture"})
        self.api.verify_device.assert_called_once_with("controller identity",node,self.a,session,{"proof":"fixture"})
        self.api.manage(session,"name","Verified")
        self.assertEqual(self.api.sync(session,"Owner")["name"],"Verified")

    def test_rotation_revokes_partially_applied_controller_grant(self):
        self.setup_groups()
        owner, peer = "d"*10, "b"*10
        for n in (owner,peer):
            self.members[n] = {"authorized":False,"tags":[],"ipAssignments":[]}
        self.api.redeem(self.api.issue(self.a),owner,"1"*64,"Owner")
        self.api.assign_owner(self.a,owner)
        self.api.manage("1"*64,"password","old")
        token = self.api.login("A","old")
        original = self.api.directory
        def interrupted(db, path, payload=None):
            value = original(db,path,payload)
            if payload == {"authorized":True}:
                raise ApiError(503,"Controller reply lost")
            return value
        from unittest.mock import patch
        with patch.object(self.api,"directory",side_effect=interrupted), self.assertRaises(ApiError):
            self.api.redeem(token,peer,"2"*64,"Peer")
        self.assertTrue(self.members[peer]["authorized"])
        self.api.manage("1"*64,"password-mode","off")
        self.assertFalse(self.members[peer]["authorized"])
