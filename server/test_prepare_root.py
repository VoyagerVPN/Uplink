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
