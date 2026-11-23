import tempfile
import unittest
from pathlib import Path

from provision import managed_file


class InstallationTest(unittest.TestCase):
    def test_repeated_install_preserves_unrelated_server_configuration(self):
        with tempfile.TemporaryDirectory() as tmp:
            config = Path(tmp) / "uplink.conf"
            self.assertTrue(managed_file(config, b"first\n", 0o644))
            self.assertFalse(managed_file(config, b"first\n", 0o644))
            config.write_bytes(b"operator configuration\n")
            with self.assertRaises(ValueError):
                managed_file(config, b"first\n", 0o644)
            self.assertEqual(config.read_bytes(), b"operator configuration\n")
