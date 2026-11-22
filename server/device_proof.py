"""Проверка подписанного устройством билета штатными ключами ZeroTier."""

import hashlib
import os
import re
import subprocess
import tempfile
from pathlib import Path

from instance import ApiError


def binary():
    if os.name == "nt":
        return Path(os.environ["ProgramData"]) / "ZeroTier/One/zerotier-one_x64.exe"
    return Path("/opt/uplink-zt/1.16.2/usr/sbin/zerotier-one")


def matches(pattern, value):
    return isinstance(value, str) and re.fullmatch(pattern, value) is not None


def verify(identity, node, challenge, session, proof):
    public = r"[0-9a-f]{10}:0:[0-9a-f]{128}"
    if (not matches(public, identity) or not matches(r"[0-9a-f]{10}", node)
            or not identity.startswith(node + ":") or not isinstance(proof, dict)
            or not matches(public, proof.get("delegate"))
            or not matches(r"[0-9a-f]{192}", proof.get("certificate"))
            or not matches(r"[0-9a-f]{192}", proof.get("signature"))
            or not matches(r"(?:[0-9a-f]{16}|[0-9a-f]{32}|[0-9a-f]{64})", challenge)
            or not matches(r"[0-9a-f]{64}", session)):
        raise ApiError(403, "Device proof required")
    delegate = proof["delegate"]
    # Билет и сессия не попадают в аргументы процессов или временный файл.
    content = "\n".join(("uplink/membership/v1", node, challenge, session)).encode("ascii")
    checks = (
        (identity, ("uplink/delegate/v1\n" + delegate).encode("ascii"), proof["certificate"]),
        (delegate, hashlib.sha256(content).hexdigest().encode("ascii"), proof["signature"]),
    )
    with tempfile.TemporaryDirectory() as directory:
        path = Path(directory) / "message"
        for signer, data, signature in checks:
            path.write_bytes(data)
            try:
                result = subprocess.run(
                    [str(binary()), "-i", "verify", signer, str(path), signature],
                    stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL,
                    cwd=directory, timeout=5, creationflags=getattr(subprocess, "CREATE_NO_WINDOW", 0),
                )
            except (OSError, subprocess.TimeoutExpired) as error:
                raise ApiError(503, "Device verification unavailable") from error
            if result.returncode:
                raise ApiError(403, "Invalid device proof")

