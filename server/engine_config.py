"""Конфигурация общего UDP-входа и изолированных сетей одного VPS."""
import ipaddress
import os
from pathlib import Path
import re
import secrets
import tempfile
import uuid


def atomic_write(path, text):
    if path.exists() and path.read_text() == text:
        return False
    with tempfile.NamedTemporaryFile(mode="w", encoding="ascii", dir=path.parent, delete=False) as out:
        pending = Path(out.name)
        try:
            out.write(text)
            out.flush()
            os.fsync(out.fileno())
        except BaseException:
            pending.unlink(missing_ok=True)
            raise
    try:
        os.replace(pending, path)
    finally:
        pending.unlink(missing_ok=True)
    return True


def secret(path):
    if not path.exists():
        atomic_write(path, secrets.token_hex(32))
    value = path.read_text()
    if not re.fullmatch("[a-f0-9]{64}", value):
        raise ValueError("Invalid saved Uplink network secret")
    return value


def configure(state, groups, port, *, host_tun=True):
    configs = state / "networks"
    configs.mkdir(mode=0o700, parents=True, exist_ok=True)
    if not 1 <= port <= 65535:
        raise ValueError("Invalid UDP port")
    names = []
    for group, subnet in sorted(groups):
        if not re.fullmatch("[a-f0-9]{32}", group):
            raise ValueError("Invalid network identifier")
        names.append(group)
        address = ipaddress.IPv4Address(int(subnet) + 1)
        key = secret(state / (group + ".secret"))
        atomic_write(configs / (group + ".toml"), f'''instance_name = "{group}"
instance_id = "{uuid.UUID(hex=group)}"
ipv4 = "{address}/24"
dhcp = false
listeners = []
stun_servers = []
routes = []
credential_file = "{state}/{group}.credentials"
[[peer]]
uri = "udp://127.0.0.1:{port}"
[network_identity]
network_name = "{group}"
network_secret = "{key}"
[secure_mode]
enabled = true
[flags]
dev_name = "up{group[:12]}"
no_tun = {str(not host_tun).lower()}
private_mode = true
enable_ipv6 = false
disable_upnp = true
bind_device = false
''')
    relay = secret(state / "relay.secret")
    atomic_write(configs / "ingress.toml", f'''instance_name = "ingress"
instance_id = "{uuid.UUID(hex=relay[:32])}"
listeners = ["udp://0.0.0.0:{port}"]
stun_servers = []
[network_identity]
network_name = "ingress-{relay[:16]}"
network_secret = "{relay}"
[secure_mode]
enabled = true
[flags]
no_tun = true
enable_ipv6 = false
disable_upnp = true
relay_network_whitelist = "{' '.join(names)}"
relay_all_peer_rpc = false
''')

