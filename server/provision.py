"""Install a private Uplink instance on a Debian 12 or Ubuntu 24.04 amd64 VPS."""
import hashlib
import http.client
import ipaddress
import json
import os
from pathlib import Path
import re
import shutil
import socket
import subprocess
import stat
import tempfile
import time
import urllib.request

APP = Path("/opt/uplink-zt")
STATE = Path("/var/lib/uplink-zt")
UNITS = Path("/etc/systemd/system")
BIN = APP / "1.16.2/usr/sbin/zerotier-one"
SOURCES = ("instance.py", "groups.py", "invites.py", "device_proof.py", "client_api.py")
SERVICES = ("uplink-zt.service", "uplink-instance.socket", "uplink-instance.service", "uplink-client-api.service")
PACKAGES = {"noble": "fe78e8d3b03fe77d1607bffd67dcd6723e2bacc4ac063afb5e311400fda12ea6",
            "bookworm": "f8927407ab99f8ac93e2ceb177f414912c24d6ca158fd65d333e7c68f0355376"}


def managed_file(path, data, mode):
    if path.is_symlink():
        raise ValueError(f"Unexpected symbolic link at {path}")
    if path.exists():
        if path.read_bytes() != data:
            raise ValueError(f"Existing configuration differs at {path}")
        return False
    with tempfile.NamedTemporaryFile(dir=path.parent, delete=False) as f:
        stage = Path(f.name)
        f.write(data)
    try:
        stage.chmod(mode)
        os.link(stage, path)
    finally:
        stage.unlink()
    return True


def run(*args, timeout=420):
    subprocess.run(args, check=True, timeout=timeout, stdin=subprocess.DEVNULL,
                   stdout=subprocess.DEVNULL, env={**os.environ, "DEBIAN_FRONTEND": "noninteractive"})


BINARY_HASHES = {
    "noble": "324ddced4cae02b59ba8fbb854899214b1b2d8b6bbed13eff897678a7d6ecd90",
    "bookworm": "f1d57895144065ce963292f658f9eac4375c42013ccdf083e37124097b612334",
}


def trusted_directory(path):
    for directory in (path, *path.parents):
        info = directory.lstat()
        if not stat.S_ISDIR(info.st_mode) or info.st_uid != 0 or info.st_mode & 0o022:
            raise ValueError("The Uplink installation directory must be owned by root")


def ensure_binary(codename, directory):
    BIN.parent.mkdir(parents=True, exist_ok=True)
    trusted_directory(BIN.parent)
    if BIN.exists():
        info = BIN.lstat()
        if (not stat.S_ISREG(info.st_mode) or info.st_uid != 0 or info.st_mode & 0o022
                or hashlib.sha256(BIN.read_bytes()).hexdigest() != BINARY_HASHES[codename]):
            raise ValueError("Unexpected ZeroTier binary")
        return
    cache = Path("/var/cache/uplink-zt") / f"zerotier-one-1.16.2-{codename}"
    if cache.is_file():
        trusted_directory(cache.parent)
        binary = cache.read_bytes()
        if hashlib.sha256(binary).hexdigest() != BINARY_HASHES[codename]:
            raise ValueError("Cached ZeroTier binary checksum mismatch")
        managed_file(BIN, binary, 0o755)
        return
    url = f"https://download.zerotier.com/debian/{codename}/pool/main/z/zerotier-one/zerotier-one_1.16.2_amd64.deb"
    package = directory / "zerotier.deb"
    with urllib.request.urlopen(url, timeout=40) as response, package.open("wb") as f:
        shutil.copyfileobj(response, f)
    if hashlib.sha256(package.read_bytes()).hexdigest() != PACKAGES[codename]:
        raise ValueError("ZeroTier package checksum mismatch")
    run("dpkg-deb", "-x", package, directory / "extract")
    managed_file(BIN, (directory / "extract/usr/sbin/zerotier-one").read_bytes(), 0o755)


def public_tls(ip):
    certbot = APP / "certbot/bin/certbot"
    if not certbot.exists() or not shutil.which("nginx"):
        run("apt-get", "update")
        run("apt-get", "install", "-y", "nginx", "python3-venv", "ca-certificates")
        run("python3", "-m", "venv", APP / "certbot")
        run(APP / "certbot/bin/pip", "install", "--no-cache-dir", "certbot>=5.4,<7")
    web = APP / "acme"
    web.mkdir(exist_ok=True, mode=0o755)
    nginx = Path("/etc/nginx/conf.d")
    managed_file(nginx / "uplink-http.conf", f"""server {{
    listen 80;
    server_name {ip};
    location /.well-known/acme-challenge/ {{ root {web}; }}
    location / {{ return 404; }}
}}
""".encode(), 0o644)
    run("nginx", "-t")
    run("systemctl", "enable", "--now", "nginx")
    run("systemctl", "reload", "nginx")
    run(certbot, "certonly", "--non-interactive", "--agree-tos", "--register-unsafely-without-email",
        "--keep-until-expiring", "--cert-name", "uplink-public", "--required-profile", "shortlived",
        "--webroot", "--webroot-path", web, "--ip-address", ip)
    # Клиент с IP-адресом может подключаться без SNI.
    managed_file(nginx / "uplink-https.conf", f"""server {{
    listen 443 ssl default_server;
    server_name {ip};
    ssl_certificate /etc/letsencrypt/live/uplink-public/fullchain.pem;
    ssl_certificate_key /etc/letsencrypt/live/uplink-public/privkey.pem;
    client_max_body_size 2m;
    location /v1/ {{ proxy_pass http://127.0.0.1:8930; proxy_http_version 1.1; }}
    location / {{ return 404; }}
}}
""".encode(), 0o644)
    run("nginx", "-t")
    run("systemctl", "reload", "nginx")
    managed_file(UNITS / "uplink-certificate.service", f"""[Service]
Type=oneshot
ExecStart={certbot} renew --quiet --cert-name uplink-public --deploy-hook "/usr/bin/systemctl reload nginx"
""".encode(), 0o644)
    managed_file(UNITS / "uplink-certificate.timer", b"""[Timer]
OnCalendar=*-*-* 00,12:00:00
RandomizedDelaySec=10m
Persistent=true
[Install]
WantedBy=timers.target
""", 0o644)
    run("systemctl", "daemon-reload")
    run("systemctl", "enable", "--now", "uplink-certificate.timer")


def install(ip, port, bind, external_tls):
    import fcntl
    import pwd
    from prepare_root import prepare_instance
    ip, bind = str(ipaddress.IPv4Address(ip)), str(ipaddress.IPv4Address(bind))
    if not 1 <= port <= 65535 or os.geteuid() != 0:
        raise ValueError("Expected an external UDP port and root access")
    if (external_tls and not ipaddress.IPv4Address(bind).is_private) or bind == "0.0.0.0" or (not external_tls and bind != "127.0.0.1"):
        raise ValueError("The proxy backend requires a private interface")
    from platform import freedesktop_os_release, machine
    distro = freedesktop_os_release().get("VERSION_CODENAME")
    if distro not in PACKAGES or machine() != "x86_64":
        raise ValueError("Expected Ubuntu 24.04 or Debian 12 on x86_64")
    if Path("/proc/1/comm").read_text().strip() != "systemd":
        raise ValueError("The VPS requires systemd")
    APP.mkdir(exist_ok=True, mode=0o755)
    trusted_directory(APP)
    with (APP / ".setup.lock").open("a") as lock, tempfile.TemporaryDirectory() as tmp:
        fcntl.flock(lock, fcntl.LOCK_EX)
        config = json.dumps([ip, port, bind, external_tls]).encode()
        marker = APP / ".setup.json"
        if not marker.exists() and any(path.exists() for path in [STATE, APP/"instance.py", *(UNITS/u for u in SERVICES)]):
            raise ValueError("Existing unmanaged Uplink instance; installation stopped")
        if not marker.exists() and not external_tls:
            for tcp in (80, 443):
                with socket.socket() as probe:
                    probe.bind(("0.0.0.0", tcp))
        managed_file(marker, config, 0o600)
        ensure_binary(distro, Path(tmp))
        try:
            pwd.getpwnam("uplink-zt")
        except KeyError:
            run("useradd", "--system", "--user-group", "--no-create-home", "--shell", "/usr/sbin/nologin", "uplink-zt")
        prepare_instance(Path("/var/lib/uplink-zt-signing"), STATE, ip, port, BIN, "uplink-zt")
        # Файл создаётся в каталоге контроллера с его UID только при начальной подготовке.
        if not (STATE/"local.conf").exists():
            account = pwd.getpwnam("uplink-zt")
            # Создание с O_EXCL/O_NOFOLLOW исключает переход по ссылке в каталоге службы.
            fd = os.open(STATE/"local.conf", os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW, 0o600)
            with os.fdopen(fd,"wb") as out:
                out.write(b'{"settings":{"portMappingEnabled":false,"allowTcpFallbackRelay":false}}')
                os.fchown(out.fileno(),account.pw_uid,account.pw_gid)
        src = Path(__file__).resolve().parent
        for name in SOURCES:
            managed_file(APP/name, (src/name).read_bytes(), 0o644)
        for name in SERVICES:
            managed_file(UNITS/name, (src/name).read_bytes(), 0o644)
        managed_file(Path("/etc/uplink-client-api.env"), f"UPLINK_BIND={bind}\n".encode(), 0o600)
        run("systemctl", "daemon-reload")
        run("systemctl", "enable", "--now", "uplink-zt.service", "uplink-instance.socket", "uplink-client-api.service")
        if not external_tls:
            ready = APP / ".https-ready"
            if not ready.exists() or not Path("/etc/letsencrypt/live/uplink-public/fullchain.pem").is_file():
                public_tls(ip)
                managed_file(ready, b"1\n", 0o600)
            else:
                run("systemctl", "start", "nginx", "uplink-certificate.timer")


class OwnerConnection(http.client.HTTPConnection):
    def connect(self):
        self.sock = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
        self.sock.settimeout(10)
        self.sock.connect("/run/uplink-instance.sock")


def owner(method, route, data):
    conn = OwnerConnection("localhost")
    try:
        conn.request(method, route, json.dumps(data), {"Content-Type":"application/json"})
        response = conn.getresponse()
        body = json.load(response)
        if response.status not in (200,201):
            raise ValueError(body.get("error", "Owner API request failed"))
        return body
    finally:
        conn.close()


def serve(request):
    group = request["group"]
    if not re.fullmatch("[a-f0-9]{32}", group):
        raise ValueError("Invalid network identifier")
    endpoint = APP / ".public-host"
    public_host = request.get("host") or request["ip"]
    if endpoint.is_file() and not request["external_tls"]:
        previous_ip, port, bind, external = json.loads((APP / ".setup.json").read_bytes())
        if previous_ip != request["ip"]:
            raise ValueError("The public IP of this Uplink instance has changed")
        public_host = endpoint.read_text().strip()
        request.update(port=port, bind=bind, external_tls=external)
    install(request["ip"], request["port"], request["bind"], request["external_tls"])
    managed_file(endpoint, public_host.encode(), 0o644)
    for attempt in range(30):
        try:
            owner("GET", "/v1/instance", {})
            break
        except (OSError, ValueError):
            if attempt == 29:
                raise
            time.sleep(0.5)
    owner("PUT", "/v1/groups/"+group, {"name":request["name"]})
    ticket = owner("POST", "/v1/networks/"+group+"/invites", {})
    print(json.dumps({"group":group,"token":ticket["token"],"host":public_host}), flush=True)
    import sys
    node = json.loads(sys.stdin.readline(512))["node"]
    if not re.fullmatch("[a-f0-9]{10}", node):
        raise ValueError("Invalid owner identifier")
    owner("PUT", "/v1/networks/"+group+"/owner", {"node":node})
    print(json.dumps(owner("POST", "/v1/networks/"+group+"/invites", {})), flush=True)
