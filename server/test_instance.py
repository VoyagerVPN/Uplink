import unittest
from instance import ApiError, network_config
import tempfile
from pathlib import Path
from invites import Invitations

class NetworkConfigurationTest(unittest.TestCase):
    def test_rejects_network_ids_that_assign_the_same_adapter_mac(self):
        from unittest.mock import patch
        from instance import dispatch
        old = "7dd1c277f3000082"
        known = {"/status":{"address":old[:10]}, "/controller/network":[old],
                 "/controller/network/"+old:{"name":"First","routes":[{"target":"10.233.130.0/24"}]}}
        directory=tempfile.TemporaryDirectory()
        self.addCleanup(directory.cleanup)
        api=Invitations(Path(directory.name)/"db", verify_device=lambda *args: None)
        with patch("invites.Invitations",return_value=api), patch("instance.controller", side_effect=lambda p,data=None: known[p] if data is None else {"id":p[-16:]}):
            with self.assertRaises(ApiError) as duplicate:
                dispatch("PUT","/v1/networks/7dd1c277f3000083",{"name":"Second","subnet":"10.233.131.0/24"})
            self.assertEqual(duplicate.exception.status,409)
            status,_=dispatch("PUT","/v1/networks/7dd1c277f30c49b6",{"name":"Second","subnet":"10.233.131.0/24"})
            self.assertEqual(status,201)
    def test_only_boolean_authorization_is_accepted(self):
        from instance import member_update
        with self.assertRaises(ApiError):
            member_update({"authorized":"false"})
        with self.assertRaises(ApiError):
            member_update({"authorized":True,"role":"owner"})
        self.assertEqual(member_update({"authorized":False}), {"authorized":False})

    def test_owner_creates_private_lan_with_managed_addresses(self):
        config = network_config({"name": "Uplink Test 129", "subnet": "10.233.129.0/24"})
        self.assertIs(config["private"], True)
        self.assertIs(config["enableBroadcast"], True)
        self.assertEqual(config["routes"], [{"target": "10.233.129.0/24", "via": None}])
        self.assertEqual(config["ipAssignmentPools"], [{"ipRangeStart":"10.233.129.20","ipRangeEnd":"10.233.129.239"}])
