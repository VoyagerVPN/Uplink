"""Управление ключами через локальный API EasyTier."""
import base64
import json
import re
import subprocess
from pathlib import Path
from instance import ApiError


class Credentials:
    def __init__(self, cli=Path("/opt/uplink/bin/easytier-cli"), portal="127.0.0.1:941"):
        self.cli, self.portal = cli, portal

    def command(self, network, node, args):
        if not re.fullmatch(r"[a-f0-9]{32}", network) or not re.fullmatch(r"[a-f0-9]{10}", node):
            raise ApiError(400, "Invalid identifier")
        try:
            result = subprocess.run([self.cli, "-p", self.portal, "-n", network, "-o", "json",
                                     "credential", *args], stdin=subprocess.DEVNULL,
                                    stdout=subprocess.PIPE, stderr=subprocess.DEVNULL,
                                    timeout=5, check=True)
        except (OSError, subprocess.SubprocessError):
            raise ApiError(503, "Network engine unavailable") from None
        return result.stdout

    def issue(self, network, node):
        result = self.command(network, node, ["generate", "--credential-id", node, "--ttl", "2592000",
                                              "--reusable", "false"])
        try:
            doc = json.loads(result)
            secret = doc["credential_secret"]
            if doc["credential_id"] != node or len(base64.b64decode(secret, validate=True)) != 32:
                raise ValueError()
            expiry = doc["expiry_unix"]
            if isinstance(expiry, str) and re.fullmatch(r"[0-9]{1,19}", expiry):
                expiry = int(expiry)
            if type(expiry) is not int or not 0 < expiry < 2**63:
                raise ValueError()
            return secret, expiry
        except (ValueError, KeyError, TypeError):
            raise ApiError(503, "Invalid network engine response") from None

    def revoke(self, network, node):
        self.command(network, node, ["revoke", node])

