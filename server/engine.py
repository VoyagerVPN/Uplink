"""Запуск набора изолированных сетей с одним внешним UDP-портом."""
import os
import signal
import subprocess
import time
from pathlib import Path
from engine_config import configure
from instance import networks


def main():
    state = Path("/var/lib/uplink-engine")
    api = networks()
    stop = False

    def terminate(*_):
        nonlocal stop
        stop = True

    signal.signal(signal.SIGTERM, terminate)
    child = None
    previous = None
    try:
        while not stop:
            with api.transaction() as db:
                groups = tuple(tuple(r) for r in db.execute("SELECT id,subnet FROM networks ORDER BY id"))
            if groups != previous:
                if child:
                    child.terminate()
                    child.wait(timeout=8)
                configure(state, groups, int(os.getenv("UPLINK_LISTEN_PORT", "9993")),
                          host_tun=Path("/dev/net/tun").is_char_device())
                child = subprocess.Popen(["/opt/uplink/bin/easytier-core", "--config-dir", state / "networks",
                                          "--rpc-portal", "127.0.0.1:941", "--rpc-portal-whitelist", "127.0.0.1/32"],
                                         stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL)
                previous = groups
            if child.poll() is not None:
                raise RuntimeError("Network engine exited")
            time.sleep(1)
    finally:
        if child and child.poll() is None:
            child.terminate()
            try:
                child.wait(timeout=8)
            except subprocess.TimeoutExpired:
                child.kill()
                child.wait()


if __name__ == "__main__":
    main()

