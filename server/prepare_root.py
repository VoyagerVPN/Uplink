"""Prepare a private ZeroTier root for a new Linux Uplink instance."""
import argparse
import errno
import ipaddress
import json
import os
from pathlib import Path
import stat
import subprocess
import tempfile


def check_bundle(bundle, endpoint):
    def check(path, is_directory=False):
        st = path.lstat()
        expected = stat.S_ISDIR if is_directory else stat.S_ISREG
        if not expected(st.st_mode) or st.st_uid != os.geteuid() or st.st_mode & 0o077:
            raise ValueError("Unexpected file type, owner or permissions in the private root")

    check(bundle, is_directory=True)
    for filename in ("identity.secret", "identity.public", "planet", "world.json"):
        check(bundle / filename)
    identity = (bundle / "identity.public").read_text().strip()
    secret = (bundle / "identity.secret").read_text().strip()
    if not identity or not secret.startswith(identity + ":"):
        raise ValueError("Private and public identities do not match")
    world = json.loads((bundle / "world.json").read_bytes())
    if (world.get("worldType") != "planet" or not world.get("signingKey_SECRET")
            or world.get("roots") != [{"identity": identity, "stableEndpoints": [endpoint]}]):
        raise ValueError("The existing root configuration does not match this endpoint")
    if not 100 <= (bundle / "planet").stat().st_size <= 65536:
        raise ValueError("Invalid planet file")


def prepare(bundle, address, port, zerotier):
    if os.name != "posix":
        raise ValueError("Run private root setup on the Linux server")
    if not 1 <= port <= 65535:
        raise ValueError("Expected an external UDP port from 1 to 65535")
    endpoint = f"{ipaddress.IPv4Address(address)}/{port}"
    if os.path.lexists(bundle):
        check_bundle(bundle, endpoint)
        return
    zerotier = zerotier.resolve(strict=True)
    if subprocess.check_output([zerotier, "-v"], timeout=10).strip() != b"1.16.2":
        raise ValueError("Expected ZeroTier One 1.16.2")
    with tempfile.TemporaryDirectory(dir=bundle.parent, prefix=".uplink-root-") as temp:
        stage = Path(temp) / "bundle"
        stage.mkdir(mode=0o700)

        def idtool(*args):
            return subprocess.check_output([zerotier, "-i", *args], cwd=stage, timeout=120)

        idtool("generate", "identity.secret", "identity.public")
        world = json.loads(idtool("initmoon", "identity.public"))
        world["worldType"] = "planet"
        world["roots"][0]["stableEndpoints"] = [endpoint]
        (stage / "world.json").write_text(json.dumps(world), encoding="utf-8")
        idtool("genmoon", "world.json")
        (stage / f"{int(world['id'], 16):016x}.moon").rename(stage / "planet")
        for path in stage.iterdir():
            path.chmod(0o600)
        check_bundle(stage, endpoint)
        try:
            stage.rename(bundle)
        except OSError as error:
            if error.errno not in (errno.EEXIST, errno.ENOTEMPTY):
                raise
            check_bundle(bundle, endpoint)


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("bundle", type=Path, help="Private directory in a trusted parent")
    parser.add_argument("address", help="Public IPv4 address for the private root")
    parser.add_argument("port", type=int, help="External UDP port forwarded to the root")
    parser.add_argument("--zerotier", type=Path,
                        default=Path("/opt/uplink-zt/1.16.2/usr/sbin/zerotier-one"))
    args = parser.parse_args()
    os.umask(0o077)
    try:
        prepare(args.bundle.absolute(), args.address, args.port, args.zerotier)
    except (OSError, ValueError, subprocess.SubprocessError) as error:
        parser.exit(1, f"Private root setup failed: {error}\n")
