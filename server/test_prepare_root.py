import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest

from device_proof import binary


@unittest.skipUnless(os.name == "posix" and binary().is_file(), "Requires the Linux ZeroTier binary")
class PrivateRootTest(unittest.TestCase):
    def test_prepare_runtime_and_repeat_with_existing_controller_state(self):
        import pwd
        with tempfile.TemporaryDirectory() as parent:
            bundle = Path(parent) / "signing"
            runtime = Path(parent) / "controller"
            account = pwd.getpwnam("nobody") if os.geteuid() == 0 else pwd.getpwuid(os.geteuid())
            command = [sys.executable, str(Path(__file__).with_name("prepare_root.py")),
                       str(bundle), "192.0.2.10", "30000", "--zerotier", str(binary()),
                       "--runtime", str(runtime), "--service-user", account.pw_name]
            result = subprocess.run(command, capture_output=True, text=True)
            self.assertEqual(result.returncode, 0, result.stderr)
            files = {"identity.secret", "identity.public", "planet"}
            self.assertEqual({p.name for p in runtime.iterdir()}, files)
            for filename in files:
                copy = runtime / filename
                self.assertEqual(copy.read_bytes(), (bundle / filename).read_bytes())
                self.assertEqual(copy.stat().st_uid, account.pw_uid)
                self.assertEqual(copy.stat().st_mode & 0o777, 0o600)
            self.assertEqual(runtime.stat().st_mode & 0o777, 0o700)
            self.assertEqual(runtime.stat().st_uid, account.pw_uid)
            network = runtime / "controller.d" / "network" / "saved.json"
            network.parent.mkdir(parents=True)
            network.write_text("saved")
            before = (runtime / "identity.secret").stat()
            subprocess.run(command, check=True, capture_output=True)
            after = (runtime / "identity.secret").stat()
            self.assertEqual((before.st_ino, before.st_mtime_ns), (after.st_ino, after.st_mtime_ns))
            self.assertEqual(network.read_text(), "saved")
            (runtime / "planet").write_bytes(b"foreign planet")
            mismatch = subprocess.run(command, capture_output=True)
            self.assertNotEqual(mismatch.returncode, 0)
            self.assertEqual((runtime / "planet").read_bytes(), b"foreign planet")
            self.assertEqual(network.read_text(), "saved")

    def test_existing_controller_without_signing_state_is_preserved(self):
        import pwd
        with tempfile.TemporaryDirectory() as parent:
            runtime = Path(parent) / "controller"
            runtime.mkdir(mode=0o700)
            (runtime / "identity.secret").write_bytes(b"existing")
            bundle = Path(parent) / "signing"
            command = [sys.executable, str(Path(__file__).with_name("prepare_root.py")),
                       str(bundle), "192.0.2.10", "30000", "--runtime", str(runtime),
                       "--service-user", pwd.getpwuid(os.geteuid()).pw_name]
            result = subprocess.run(command, capture_output=True)
            self.assertNotEqual(result.returncode, 0)
            self.assertFalse(bundle.exists())
            self.assertEqual((runtime / "identity.secret").read_bytes(), b"existing")

    def test_repeating_setup_preserves_the_private_root(self):
        with tempfile.TemporaryDirectory() as parent:
            bundle = Path(parent) / "root"
            command = [sys.executable, str(Path(__file__).with_name("prepare_root.py")),
                       str(bundle), "192.0.2.10", "30000", "--zerotier", str(binary())]
            first = subprocess.run(command, capture_output=True, text=True)
            self.assertEqual(first.returncode, 0, first.stderr)
            before = {p.name: p.read_bytes() for p in bundle.iterdir()}
            self.assertEqual(set(before), {"identity.secret", "identity.public", "world.json", "planet"})
            self.assertEqual(json.loads(before["world.json"])["roots"][0]["stableEndpoints"], ["192.0.2.10/30000"])
            self.assertEqual(bundle.stat().st_mode & 0o077, 0)
            self.assertTrue(all(p.stat().st_mode & 0o077 == 0 for p in bundle.iterdir()))
            subprocess.run(command, check=True, capture_output=True)
            command[4] = "30001"
            self.assertNotEqual(subprocess.run(command, capture_output=True).returncode, 0)
            self.assertEqual({p.name: p.read_bytes() for p in bundle.iterdir()}, before)
            (bundle / "identity.public").unlink()
            command[4] = "30000"
            self.assertNotEqual(subprocess.run(command, capture_output=True).returncode, 0)
            self.assertEqual((bundle / "identity.secret").read_bytes(), before["identity.secret"])
