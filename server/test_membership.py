import tempfile
import unittest
from pathlib import Path

from instance import ApiError
from membership import Networks

class Keys:
    def __init__(self):
        self.active = {}

    def issue(self, network, node):
        import base64, secrets
        return self.active.setdefault((network, node), (base64.b64encode(secrets.token_bytes(32)).decode(), 2_000_000_000))

    def revoke(self, network, node):
        self.active.pop((network, node), None)

class MembershipTest(unittest.TestCase):
    def test_invitation_grants_one_member_and_leave_revokes_only_that_key(self):
        with tempfile.TemporaryDirectory() as tmp:
            engine = Keys()
            api = Networks(Path(tmp) / "networks.db", engine)
            group = "a1" * 16
            api.create_group(group, {"name": "LAN"})
            first = api.issue(group)
            session = "37" * 32
            grant = api.redeem(first, session, "Alice")
            self.assertEqual(grant["id"], group)
            self.assertEqual(api.redeem(first, session, "Alice"), grant)
            with self.assertRaises(ApiError):
                api.redeem(first, "26" * 32, "Bob")
            other = api.redeem(api.issue(group), "41" * 32, "Bob")
            self.assertNotEqual(grant["credential"], other["credential"])
            self.assertNotEqual(grant["ip"], other["ip"])
            self.assertEqual(len(api.sync(session, "Alice")["members"]), 2)
            api.leave(session)
            api.leave(session)
            self.assertEqual(set(engine.active), {(group, other["node"])})
            with self.assertRaises(ApiError):
                api.redeem(first, session, "Alice")


    def test_saved_member_recovers_after_six_months_until_revoked(self):
        from unittest.mock import Mock, patch
        import base64, json, secrets
        from easytier import Credentials
        from client_api import route

        clock = [1_800_000_000]
        active = {}
        def generate(network, node, args):
            if args[0] == "revoke":
                active.pop(node, None)
                return b"{}"
            if node not in active or active[node]["expiry_unix"] <= clock[0]:
                active[node] = dict(credential_id=node, expiry_unix=clock[0]+30*86400,
                                    credential_secret=base64.b64encode(secrets.token_bytes(32)).decode())
            return json.dumps(active[node]).encode()

        engine = Credentials()
        engine.command = Mock(side_effect=generate)
        with tempfile.TemporaryDirectory() as tmp:
            api = Networks(Path(tmp)/"networks.db", engine, lambda: clock[0])
            group, owner, friend = "a1"*16, "b2"*32, "c3"*32
            api.create_group(group, {"name":"Friends"})
            me = api.redeem(api.issue(group), owner, "Owner")
            api.assign_owner(group, me["node"])
            ticket = api.issue(group)
            grant = api.redeem(ticket, friend, "Friend")
            api.manage(owner, "role", grant["node"])
            with patch("client_api.networks", return_value=api), patch.dict("os.environ", UPLINK_UDP_ENDPOINT="127.0.0.1:9993"):
                def sync(expires):
                    return route("POST", "/v1/sync", dict(session=friend,nick="Friend",
                                 network=group,credential_expires_at=expires))[1]
                calls = engine.command.call_count
                self.assertNotIn("transport_profile",sync(grant["credential_expires_at"]))
                self.assertEqual(engine.command.call_count,calls)
                clock[0] += 183*86400
                with self.assertRaises(ApiError): api.lookup(ticket)
                snapshot = sync(grant["credential_expires_at"])
                new = snapshot["transport_profile"]
                self.assertEqual((new["node"],new["ip"]),(grant["node"],grant["ip"]))
                self.assertNotEqual(new["credential"],grant["credential"])
                self.assertGreater(new["credential_expires_at"],clock[0])
                self.assertEqual(new["endpoint"],"127.0.0.1:9993")
                self.assertEqual(next(m["role"] for m in snapshot["members"] if m["node"]==new["node"]),"admin")
                self.assertEqual(sync(0)["transport_profile"],new)
                self.assertNotIn("transport_profile",sync(new["credential_expires_at"]))
                api.manage(owner,"remove",new["node"])
                with self.assertRaises(ApiError): sync(0)
                with self.assertRaises(ApiError): api.profile(friend)
                self.assertNotIn(new["node"],active)
                api.leave(owner)
                with self.assertRaises(ApiError): api.profile(owner)

    def test_network_roles_and_login_are_scoped_to_the_network(self):
        with tempfile.TemporaryDirectory() as tmp:
            api = Networks(Path(tmp) / "networks.db", Keys())
            net, other = "ab" * 16, "cd" * 16
            api.create_group(net, {"name": "Friday"})
            with self.assertRaises(ApiError):
                api.create_group(other, {"name": "friday"})
            owner, guest = "12" * 32, "34" * 32
            original = api.redeem(api.issue(net), owner, "Owner")
            api.assign_owner(net, original["node"])
            api.manage(owner, "password", "invite by name")
            member = api.redeem(api.login("FRIDAY", "invite by name"), guest, "Guest")
            with self.assertRaises(ApiError):
                api.manage(guest, "name", "No")
            api.manage(owner, "role", member["node"])
            api.manage(guest, "name", "Weekend")
            with self.assertRaises(ApiError):
                api.login("Friday", "invite by name")
            self.assertEqual(api.lookup(api.login("Weekend", "invite by name"))["id"], net)
            api.manage(owner, "remove", member["node"])
            with self.assertRaises(ApiError):
                api.profile(guest)

    def test_concurrent_claim_has_one_winner_and_expired_ticket_is_denied(self):
        from concurrent.futures import ThreadPoolExecutor
        with tempfile.TemporaryDirectory() as tmp:
            clock = [1000]
            api = Networks(Path(tmp)/"state.db", Keys(), lambda: clock[0])
            group = "ab"*16
            api.create_group(group, {"name":"Test"})
            ticket = api.issue(group)
            def claim(n):
                try:
                    return api.redeem(ticket, str(n)*64, "Member")["node"]
                except ApiError as error:
                    self.assertEqual(error.status, 403)
            with ThreadPoolExecutor(2) as pool:
                self.assertEqual(sum(bool(n) for n in pool.map(claim,[1,2])),1)
            ticket = api.issue(group)
            clock[0] += 3600
            with self.assertRaises(ApiError):
                api.lookup(ticket)

    def test_password_rotation_and_disabled_link_invalidate_unclaimed_tickets(self):
        with tempfile.TemporaryDirectory() as tmp:
            api = Networks(Path(tmp)/"state.db",Keys())
            group, session = "21"*16, "c1"*32
            api.create_group(group, {"name":"Test"})
            owner = api.redeem(api.issue(group), session, "Owner")
            api.assign_owner(group,owner["node"])
            link=api.share(session)
            api.manage(session,"link","off")
            with self.assertRaises(ApiError): api.lookup(link)
            api.manage(session,"qr","on")
            self.assertEqual(api.lookup(api.share(session))["id"],group)
            api.manage(session,"password","first")
            ticket=api.login("test","first")
            api.manage(session,"password","second")
            with self.assertRaises(ApiError): api.lookup(ticket)
            with self.assertRaises(ApiError): api.login("test","first")

    def test_slow_key_issue_does_not_block_another_membership(self):
        import threading
        from concurrent.futures import ThreadPoolExecutor
        entered, release = threading.Event(), threading.Event()
        class Slow(Keys):
            def issue(self, network, node):
                if not entered.is_set():
                    entered.set()
                    if not release.wait(5):
                        raise RuntimeError("independent join blocked")
                return super().issue(network, node)
        with tempfile.TemporaryDirectory() as tmp, ThreadPoolExecutor(2) as pool:
            api = Networks(Path(tmp)/"state.db",Slow())
            group="ab"*16
            api.create_group(group,{"name":"LAN"})
            first, other=api.issue(group),api.issue(group)
            attempt=pool.submit(api.redeem,first,"a"*64,"A")
            try:
                self.assertTrue(entered.wait(2))
                self.assertEqual(api.redeem(other,"b"*64,"B")["id"],group)
            finally:
                release.set()
            self.assertEqual(attempt.result()["id"],group)

class ProvisionReadinessTest(unittest.TestCase):
    def test_waits_for_the_requested_network_before_issuing_an_invitation(self):
        from unittest.mock import Mock, patch
        import provision
        group = "ac" * 16
        with patch.object(provision.subprocess, "run", side_effect=[Mock(returncode=1), Mock(returncode=0)]) as command, patch.object(provision.time, "sleep") as sleep:
            provision.wait_for_network(group)
        self.assertEqual(command.call_count, 2)
        self.assertIn(group, command.call_args.args[0])
        sleep.assert_called_once()
