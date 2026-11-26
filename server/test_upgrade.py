import json
import os
from pathlib import Path
import sqlite3
import tempfile
import unittest
from unittest.mock import patch

import provision
import upgrade


@unittest.skipUnless(os.name == "posix", "The server updater runs on Linux")
class UpdateTest(unittest.TestCase):
    def setUp(self):
        root = Path(self.enterContext(tempfile.TemporaryDirectory()))
        self.app, self.units, self.members, self.engine, self.bundle, self.job = [root / name for name in (
            "app", "units", "members", "engine", "bundle", "0123456789abcdef0123456789abcdef",
        )]
        for path in self.app, self.units, self.members, self.engine, self.bundle, self.job:
            path.mkdir()
        (self.app / ".public-host").write_text("lan.example")
        (self.app / ".setup.json").write_text('["192.0.2.10",9993,"127.0.0.1",false]')
        self.db = self.members / "networks.sqlite"
        with sqlite3.connect(self.db) as db:
            db.execute("CREATE TABLE invites (token TEXT)")
            db.execute("INSERT INTO invites VALUES ('saved-invitation')")
        (self.engine / "identity").write_text("saved engine identity")
        for name in provision.SOURCES + provision.SERVICES:
            if name == "revision.txt":
                (self.bundle / name).write_text("3\n")
                continue
            (self.bundle / name).write_text("# next version\n")
            ((self.app if name in provision.SOURCES else self.units) / name).write_text("# old version\n")
        self.enterContext(patch.multiple(upgrade, APP=self.app, UNITS=self.units, STATES=(self.members, self.engine)))
        self.enterContext(patch.object(upgrade, "trusted_directory"))
        self.enterContext(patch.object(upgrade, "check_engine"))
        self.stops = self.enterContext(patch.object(upgrade, "stop"))
        self.starts = self.enterContext(patch.object(upgrade, "start"))

    def apply(self, host="lan.example"):
        upgrade.execute(self.bundle, self.job, host, 3)
        return [json.loads(line) for line in (self.job / "events.jsonl").read_text().splitlines()]

    def test_success_keeps_existing_network_data_and_records_the_new_version(self):
        with patch.object(upgrade, "healthy"):
            events = self.apply()
        self.assertEqual((self.app / "membership.py").read_text(), "# next version\n")
        self.assertEqual((self.app / "revision.txt").read_text().strip(), "3")
        with sqlite3.connect(self.db) as db:
            self.assertEqual(db.execute("SELECT token FROM invites").fetchone(), ("saved-invitation",))
        self.assertEqual((self.engine / "identity").read_text(), "saved engine identity")
        self.assertEqual([event["stage"] for event in events], ["prepare", "backup", "apply", "verify", "done"])
        self.assertEqual(events[-1]["result"], "updated")

    def test_a_failed_health_check_restores_program_files_and_mutated_state(self):
        (self.app / "revision.txt").write_text("2\n")

        def probe(_, revision=None):
            if (self.app / "membership.py").read_text() == "# next version\n":
                with sqlite3.connect(self.db) as db:
                    db.execute("DROP TABLE invites")
                (self.engine / "identity").write_text("changed during startup")
                raise ValueError("Failed test health check")

        with patch.object(upgrade, "healthy", side_effect=probe):
            events = self.apply()
        self.assertEqual(events[-1]["result"], "restored")
        self.assertIn({"stage": "verify", "failure": "Failed test health check"},
                      [{k: v for k, v in event.items() if k != "time"} for event in events])
        self.assertEqual((self.app / "revision.txt").read_text(), "2\n")
        self.assertEqual((self.units / "uplink-api.service").read_text(), "# old version\n")
        with sqlite3.connect(self.db) as db:
            self.assertEqual(db.execute("SELECT token FROM invites").fetchone(), ("saved-invitation",))
        self.assertEqual((self.engine / "identity").read_text(), "saved engine identity")
        self.assertFalse((self.job.parent / "pending").exists())

    def test_the_wrong_host_is_rejected_before_stopping_the_services(self):
        events = self.apply("other.example")
        self.assertEqual(events[-1]["result"], "failed")
        self.stops.assert_not_called()
        self.assertEqual((self.app / "membership.py").read_text(), "# old version\n")

    def test_an_older_client_cannot_downgrade_a_newer_vps(self):
        (self.app / "revision.txt").write_text("7\n")
        events = self.apply()
        self.assertEqual(events[-1]["result"], "failed")
        self.stops.assert_not_called()
        self.assertEqual((self.app / "revision.txt").read_text(), "7\n")
