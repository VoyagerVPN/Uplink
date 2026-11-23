"""Read one installer bundle from the authenticated SSH stream."""
import base64
import json
import os
from pathlib import Path
import sys
import tempfile
import zlib


def main():
    files = {"provision.py", "prepare_root.py", "instance.py", "groups.py", "invites.py",
             "device_proof.py", "client_api.py", "uplink-zt.service", "uplink-client-api.service",
             "uplink-instance.service", "uplink-instance.socket"}
    if os.geteuid() != 0:
        raise ValueError("Administrator access is required on the VPS.")
    print(json.dumps({"stage":"authenticated"}), flush=True)
    packet = json.loads(zlib.decompress(base64.b64decode(sys.stdin.buffer.readline(512 * 1024), validate=False)))
    if set(packet["files"]) != files:
        raise ValueError("Invalid installation bundle or insufficient privileges")
    with tempfile.TemporaryDirectory(prefix="uplink-", dir="/run") as directory:
        for name, contents in packet["files"].items():
            (Path(directory)/name).write_text(contents, encoding="utf-8")
        sys.path.insert(0, directory)
        from provision import serve
        serve(packet["request"])


try:
    main()
except Exception as error:
    print(json.dumps({"error": str(error)[:350]}), flush=True)
    sys.exit(1)
