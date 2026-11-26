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
    def test_ssh_request_binds_one_public_key_and_keeps_it_private_to_the_parties(self):
        import base64
        from unittest.mock import patch
        from client_api import route

        def identity(byte):
            wire = b"\0\0\0\x0bssh-ed25519\0\0\0\x20" + bytes([byte]) * 32
            return "ssh-ed25519 " + base64.b64encode(wire).decode("ascii")

        with tempfile.TemporaryDirectory() as tmp:
            api = Networks(Path(tmp) / "networks.db", Keys())
            group = "ab" * 16
            alice, bob, charlie = (digit * 64 for digit in "123")
            api.create_group(group, {"name": "Test"})
            api.redeem(api.issue(group), alice, "Alice")
            target = api.redeem(api.issue(group), bob, "Bob")
            api.redeem(api.issue(group), charlie, "Charlie")
            with patch("client_api.networks", return_value=api):
                def request(public_key):
                    return route("POST", "/v1/ssh/request", {
                        "session": alice, "recipient": target["node"], "public_key": public_key,
                    })[1]

                with self.assertRaises(ApiError):
                    request('command="ignored" ' + identity(1))
                legacy = api.request_ssh(alice, target["node"])
                first = request(identity(1))
                self.assertNotEqual(first["id"], legacy["id"])
                self.assertEqual(first["public_key"], identity(1))
                self.assertEqual(request(identity(1))["id"], first["id"])
                with self.assertRaises(ApiError) as conflict:
                    request(identity(2))
                self.assertEqual(conflict.exception.status, 409)
                self.assertEqual(api.sync(bob, "Bob", include_ssh=True)["ssh_requests"][0]["public_key"], identity(1))
                self.assertEqual(api.sync(charlie, "Charlie", include_ssh=True)["ssh_requests"], [])
                api.respond_ssh(bob, first["id"], "reject")
                replacement = request(identity(2))
                self.assertNotEqual(replacement["id"], first["id"])
                self.assertEqual(replacement["public_key"], identity(2))

    def test_an_ssh_request_is_visible_only_to_the_two_current_network_members(self):
        from unittest.mock import patch
        from client_api import route

        with tempfile.TemporaryDirectory() as tmp:
            api = Networks(Path(tmp) / "networks.db", Keys(), lambda: 1000)
            group, outside = "ab" * 16, "cd" * 16
            api.create_group(group, {"name": "Test"})
            api.create_group(outside, {"name": "Other"})
            alice, bob, charlie, dana = (digit * 64 for digit in "1234")
            source = api.redeem(api.issue(group), alice, "Alice")
            target = api.redeem(api.issue(group), bob, "Bob")
            api.redeem(api.issue(group), charlie, "Charlie")
            other = api.redeem(api.issue(outside), dana, "Dana")

            with patch("client_api.networks", return_value=api):
                def post(path, **body):
                    status, result = route("POST", "/v1/" + path, body)
                    self.assertEqual(status, 200)
                    return result

                request = post("ssh/request", session=alice, recipient=target["node"])
                self.assertEqual(request["requester"], source["node"])
                self.assertEqual(request["recipient"], target["node"])
                self.assertEqual(request["state"], "pending")
                self.assertEqual(post("ssh/request", session=alice, recipient=target["node"]), request)
                self.assertEqual(post("sync", session=bob, nick="Bob", ssh_requests=True)["ssh_requests"], [request])
                self.assertEqual(post("sync", session=alice, nick="Alice", ssh_requests=True)["ssh_requests"], [request])
                self.assertEqual(post("sync", session=charlie, nick="Charlie", ssh_requests=True)["ssh_requests"], [])
                self.assertNotIn("ssh_requests", post("sync", session=bob, nick="Bob"))

                for session, recipient in ((alice, source["node"]), (alice, other["node"]), ("5" * 64, target["node"])):
                    with self.assertRaises(ApiError):
                        post("ssh/request", session=session, recipient=recipient)

                for session, decision in ((alice, "reject"), (bob, "cancel"), (charlie, "reject"), (bob, "approve")):
                    with self.assertRaises(ApiError):
                        post("ssh/respond", session=session, id=request["id"], decision=decision)

                rejected = post("ssh/respond", session=bob, id=request["id"], decision="reject")
                self.assertEqual(rejected["state"], "rejected")
                self.assertEqual(post("ssh/respond", session=bob, id=request["id"], decision="reject"), rejected)
                self.assertEqual(post("sync", session=alice, nick="Alice", ssh_requests=True)["ssh_requests"], [rejected])

                renewed = post("ssh/request", session=alice, recipient=target["node"])
                self.assertNotEqual(renewed["id"], request["id"])
                with self.assertRaises(ApiError):
                    post("ssh/respond", session=bob, id=request["id"], decision="reject")
                cancelled = post("ssh/respond", session=alice, id=renewed["id"], decision="cancel")
                self.assertEqual(cancelled["state"], "cancelled")
                snapshot = post("sync", session=bob, nick="Bob", ssh_requests=True)
                self.assertEqual(snapshot["ssh_requests"], [cancelled])
                self.assertEqual([(entry["id"], entry["state"], entry["occurred_at"])
                                  for entry in snapshot["ssh_history"]],
                                 [(renewed["id"], "cancelled", 1000), (request["id"], "rejected", 1000)])
                self.assertEqual(post("sync", session=charlie, nick="Charlie", ssh_requests=True)["ssh_history"], [])

                api.leave(alice)
                self.assertEqual(post("sync", session=bob, nick="Bob", ssh_requests=True)["ssh_requests"], [])

    def test_concurrent_ssh_cancel_and_rejection_leave_one_terminal_state(self):
        from concurrent.futures import ThreadPoolExecutor

        with tempfile.TemporaryDirectory() as tmp:
            api = Networks(Path(tmp) / "networks.db", Keys())
            group, alice, bob = "ab" * 16, "1" * 64, "2" * 64
            api.create_group(group, {"name": "Test"})
            api.redeem(api.issue(group), alice, "Alice")
            target = api.redeem(api.issue(group), bob, "Bob")
            request = api.request_ssh(alice, target["node"])

            def close(session, decision):
                try:
                    return api.respond_ssh(session, request["id"], decision)["state"]
                except ApiError as error:
                    return error.status

            with ThreadPoolExecutor(2) as pool:
                states = [pool.submit(close, alice, "cancel"), pool.submit(close, bob, "reject")]
                results = [state.result() for state in states]
            self.assertEqual(results.count(409), 1)
            terminal, = [state for state in results if isinstance(state, str)]
            self.assertIn(terminal, ("cancelled", "rejected"))
            self.assertEqual(api.sync(alice, "Alice", include_ssh=True)["ssh_requests"][0]["state"], terminal)

    def test_network_nickname_survives_sync_and_cannot_rename_an_unrelated_member(self):
        with tempfile.TemporaryDirectory() as tmp:
            api = Networks(Path(tmp) / "networks.db", Keys())
            group, outside = "ab" * 16, "cd" * 16
            alice, bob, other = (digit * 64 for digit in "123")
            api.create_group(group, {"name": "Test"})
            api.create_group(outside, {"name": "Other"})
            owner = api.redeem(api.issue(group), alice, "Alice")
            guest = api.redeem(api.issue(group), bob, "Bob")
            unrelated = api.redeem(api.issue(outside), other, "Bob")
            api.assign_owner(group, owner["node"])
            self.assertEqual(api.lookup(api.share(bob))["id"], group)
            with self.assertRaises(ApiError):
                api.rename_member(bob, owner["node"], "No")
            with self.assertRaises(ApiError):
                api.rename_member(alice, unrelated["node"], "No")
            api.rename_member(bob, guest["node"], "Local Bob")
            api.rename_member(alice, guest["node"], "Co-op Bob")
            names = {p["node"]: p["nick"] for p in api.sync(bob, "Global Bob")["members"]}
            self.assertEqual(names[guest["node"]], "Co-op Bob")
            self.assertEqual(api.sync(other, "Other Bob")["members"][0]["nick"], "Other Bob")

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
