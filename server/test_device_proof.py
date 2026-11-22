import hashlib
import subprocess
import tempfile
import unittest
from pathlib import Path

import device_proof
from instance import ApiError


class DeviceProofTest(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        binary = device_proof.binary()
        if not binary.is_file():
            raise unittest.SkipTest("ZeroTier executable is unavailable")
        cls.directory = tempfile.TemporaryDirectory()
        cls.addClassCleanup(cls.directory.cleanup)
        folder = Path(cls.directory.name)
        cls.binary = binary
        def run(*args):
            return subprocess.check_output([str(binary), "-i", *map(str, args)],
                stderr=subprocess.DEVNULL, creationflags=getattr(subprocess, "CREATE_NO_WINDOW", 0)).strip().decode("ascii")
        for who in ("node", "delegate"):
            run("generate", folder / who, folder / (who + ".pub"))
        cls.identity = (folder / "node.pub").read_text().strip()
        cls.node = cls.identity.split(":")[0]
        delegate = (folder / "delegate.pub").read_text().strip()
        msg = folder / "message"
        msg.write_bytes(("uplink/delegate/v1\n" + delegate).encode("ascii"))
        cert = run("sign", folder / "node", msg)
        cls.ticket, cls.session = "e"*64, "f"*64
        payload = "\n".join(("uplink/membership/v1", cls.node, cls.ticket, cls.session)).encode("ascii")
        msg.write_bytes(hashlib.sha256(payload).hexdigest().encode("ascii"))
        cls.proof = dict(delegate=delegate, certificate=cert, signature=run("sign", folder / "delegate", msg))

    def test_only_the_bound_device_ticket_and_session_are_accepted(self):
        verify = device_proof.verify
        verify(self.identity, self.node, self.ticket, self.session, self.proof)
        for node, ticket, session, proof in [
            ("a"*10, self.ticket, self.session, self.proof),
            (self.node, "1"*64, self.session, self.proof),
            (self.node, self.ticket, "2"*64, self.proof),
            (self.node, self.ticket, self.session, {**self.proof, "certificate": "0"*192}),
            (self.node, self.ticket, self.session, None),
        ]:
            with self.subTest(node=node), self.assertRaises(ApiError) as denied:
                verify(self.identity, node, ticket, session, proof)
            self.assertEqual(denied.exception.status, 403)

