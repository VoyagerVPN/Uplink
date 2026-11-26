"""Update a managed instance in place and restore its previous state on failure."""
from contextlib import contextmanager
import http.client
import ipaddress
import json
import os
from pathlib import Path
import re
import shutil
import stat
import subprocess
import sys
import tempfile
import time
import uuid

sys.path.insert(0, str(Path(__file__).resolve().parent))
from provision import APP, ENGINE_FILES, SERVICES, SOURCES, STATE, UNITS, owner, trusted_directory


STATES = (Path("/var/lib/uplink-members"), STATE)
WORK = Path("/var/lib/uplink-upgrade")
UNIT = "uplink-server-upgrade.service"
ENABLED = ("uplink-engine.service", "uplink-owner.socket", "uplink-api.service")


def event(job, stage, **fields):
    value = {"stage": stage, "time": int(time.time()), **fields}
    with (job / "events.jsonl").open("a", encoding="utf-8") as log:
        log.write(json.dumps(value) + "\n")
        log.flush()
        os.fsync(log.fileno())


def sync_dir(directory):
    fd = os.open(directory, os.O_RDONLY | os.O_DIRECTORY)
    try:
        os.fsync(fd)
    finally:
        os.close(fd)


def atomic_write(path, contents):
    file = tempfile.NamedTemporaryFile(dir=path.parent, delete=False)
    staged = Path(file.name)
    try:
        with file:
            file.write(contents)
            file.flush()
            os.fsync(file.fileno())
        staged.chmod(0o644)
        os.replace(staged, path)
        sync_dir(path.parent)
    finally:
        staged.unlink(missing_ok=True)


@contextmanager
def lock(path):
    import fcntl
    with os.fdopen(os.open(path, os.O_CREAT | os.O_RDWR | os.O_NOFOLLOW, 0o600), "r+") as file:
        fcntl.flock(file, fcntl.LOCK_EX)
        yield


def regular(path):
    if not stat.S_ISREG(path.lstat().st_mode):
        raise ValueError(f"Expected a regular file: {path.name}")
    return path


def preflight(host, version):
    if type(version) is not int or not 1 <= version <= 65535:
        raise ValueError("Invalid server revision")
    trusted_directory(APP)
    trusted_directory(UNITS)
    actual = regular(APP / ".public-host").read_text().strip()
    if not isinstance(host, str) or host.lower() != actual.lower():
        raise ValueError("This VPS does not serve the selected network address")
    _, _, bind, _ = json.loads(regular(APP / ".setup.json").read_text())
    ipaddress.IPv4Address(bind)
    return bind


def check_engine():
    import hashlib
    trusted_directory(APP / "bin")
    for name, (_, expected) in ENGINE_FILES.items():
        path = regular(APP / "bin" / name)
        if path.stat().st_uid != 0 or path.stat().st_mode & 0o022 or hashlib.sha256(path.read_bytes()).hexdigest() != expected:
            raise ValueError("The installed network engine does not match this update")


def command(*args, **kwargs):
    return subprocess.run(args, timeout=kwargs.pop("timeout", 60), stdin=subprocess.DEVNULL,
                          stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, **kwargs)


def stop():
    command("systemctl", "stop", *SERVICES, check=True)


def start():
    command("systemctl", "daemon-reload", check=True)
    command("systemctl", "start", *ENABLED, check=True)


def healthy(bind, revision=None):
    deadline = time.monotonic() + 15
    while True:
        try:
            for service in ENABLED:
                check = service
                command("systemctl", "is-active", "--quiet", service, check=True, timeout=5)
            check = "local owner API"
            groups = owner("GET", "/v1/instance", {})["networks"]
            check = "EasyTier network credentials"
            for group in groups:
                if not re.fullmatch("[a-f0-9]{32}", group):
                    raise ValueError("Unexpected network identifier")
                command(APP / "bin/easytier-cli", "-p", "127.0.0.1:941", "-n", group,
                        "credential", "list", check=True, timeout=2)
            check = "local client API"
            connection = http.client.HTTPConnection(bind, 8930, timeout=2)
            try:
                connection.request("POST", "/v1/ping", b"{}", {"Content-Type": "application/json"})
                reply = connection.getresponse()
                body = json.loads(reply.read(1024))
                if reply.status == 200 and isinstance(body, dict) and (revision is None or body.get("server_revision") == revision):
                    return
                raise ValueError(f"HTTP {reply.status} or unexpected server revision")
            finally:
                connection.close()
        except (OSError, ValueError, http.client.HTTPException, subprocess.SubprocessError) as error:
            failure = describe(error)
        if time.monotonic() >= deadline:
            raise ValueError(f"Health check failed at {check}: {failure}")
        time.sleep(0.5)


def files():
    return [*(APP / name for name in SOURCES), *(UNITS / name for name in SERVICES)]


def copy(source, target):
    command("cp", "-a", "--", source, target, check=True, timeout=180)


def describe(error):
    if isinstance(error, subprocess.CalledProcessError):
        return f"{error.cmd[0]} {error.cmd[1]} exited with code {error.returncode}"
    if isinstance(error, subprocess.TimeoutExpired):
        return f"{error.cmd[0]} {error.cmd[1]} timed out"
    if isinstance(error, OSError):
        return f"{type(error).__name__} (errno {error.errno})"
    if isinstance(error, ValueError):
        return str(error)[:220]
    return type(error).__name__


def save(backup):
    backup.mkdir(mode=0o700)
    for path in files():
        if path.name == "revision.txt" and not path.exists() and not path.is_symlink():
            continue
        copy(regular(path), backup / path.name)
    for state in STATES:
        if not stat.S_ISDIR(state.lstat().st_mode) or state.is_mount():
            raise ValueError(f"Unsupported state directory: {state.name}")
        copy(state, backup / state.name)
    command("sync", "-f", backup, check=True, timeout=180)


def restore(backup):
    for path in files():
        saved = backup / path.name
        if saved.exists():
            atomic_write(path, saved.read_bytes())
        else:
            path.unlink(missing_ok=True)
    for state in STATES:
        with tempfile.TemporaryDirectory(dir=state.parent, prefix=".uplink-restore-") as temp:
            temp = Path(temp)
            copy(backup / state.name, temp / "restore")
            if state.exists():
                os.replace(state, temp / "previous")
            try:
                os.replace(temp / "restore", state)
                sync_dir(state.parent)
            except OSError:
                if not state.exists() and (temp / "previous").exists():
                    os.replace(temp / "previous", state)
                raise


def execute(bundle, job, host, version):
    pending = job.parent / "pending"
    backup = job / "backup"
    maintenance = APP / ".update-in-progress"
    stage = "prepare"
    stopped = False
    finished = False
    recovering = False
    try:
        event(job, stage, target=version)
        bind = preflight(host, version)
        if pending.exists():
            recovering = True
            stage = "rollback"
            previous = pending.read_text().strip()
            if not re.fullmatch(r"[a-f0-9]{32}", previous):
                raise ValueError("The previous update needs manual recovery")
            backup = job.parent / previous / "backup"
            event(job, "rollback", detail="Recovering a previously interrupted update")
            stop()
            restore(backup)
            start()
            healthy(bind)
            pending.unlink()
            sync_dir(pending.parent)
            maintenance.unlink(missing_ok=True)
            event(job, "done", result="restored", detail="The interrupted update was rolled back")
            finished = True
            return
        check_engine()
        revision = APP / "revision.txt"
        current = int(regular(revision).read_text()) if revision.exists() or revision.is_symlink() else None
        if current is not None and current > version:
            raise ValueError("This server is already running a newer version")
        sources = {path: regular(bundle / path.name).read_bytes() for path in files()}
        if int(sources[revision]) != version:
            raise ValueError("The update bundle does not match its server revision")
        for path, source in sources.items():
            if path != revision or current is not None:
                regular(path)
            if path.suffix == ".py":
                compile(source, path.name, "exec")
        if current == version and all(path.read_bytes() == source for path, source in sources.items()):
            healthy(bind, version)
            maintenance.unlink(missing_ok=True)
            event(job, "done", result="current", version=version)
            finished = True
            return
        stage = "backup"
        event(job, stage, source=current if current is not None else "unversioned")
        stopped = True
        stop()
        save(backup)
        atomic_write(pending, job.name.encode())
        atomic_write(maintenance, b"\n")
        stage = "apply"
        event(job, stage)
        for path, source in sources.items():
            atomic_write(path, source)
        start()
        stage = "verify"
        event(job, stage)
        healthy(bind, version)
        pending.unlink()
        sync_dir(pending.parent)
        maintenance.unlink()
        event(job, "done", result="updated", version=version)
        finished = True
    except Exception as error:
        detail = describe(error)
        event(job, stage, failure=detail)
        if not stopped:
            result = "restore_failed" if recovering else "failed"
        else:
            event(job, "rollback", detail=detail)
            try:
                if pending.exists():
                    stop()
                    restore(backup)
                start()
                healthy(bind)
                pending.unlink(missing_ok=True)
                sync_dir(pending.parent)
                maintenance.unlink(missing_ok=True)
                result, finished = "restored", True
            except Exception as recovery_error:
                result = "restore_failed"
                event(job, "rollback", failure=describe(recovery_error))
        event(job, "done", result=result, detail=detail)
    finally:
        if finished and backup.is_dir():
            shutil.rmtree(backup)


def active():
    status = subprocess.run(["systemctl", "show", "--property=ActiveState", "--value", UNIT],
                            stdin=subprocess.DEVNULL, capture_output=True, timeout=5)
    return status.stdout.strip() in (b"active", b"activating", b"reloading")


def launch(bundle, request):
    host, version = request["host"], request["version"]
    trusted_directory(APP)
    WORK.mkdir(mode=0o700, exist_ok=True)
    trusted_directory(WORK)
    with lock(WORK / ".launch.lock"):
        current = WORK / "current"
        if active():
            identifier = current.read_text().strip()
            if not re.fullmatch(r"[a-f0-9]{32}", identifier):
                raise ValueError("Could not locate the current update")
            job = WORK / identifier
            if json.loads((job / "request.json").read_text()) != [host, version]:
                raise ValueError("Another update is running on this VPS")
        else:
            job = WORK / uuid.uuid4().hex
            job.mkdir(mode=0o700)
            shutil.copytree(bundle, job / "bundle")
            atomic_write(job / "request.json", json.dumps([host, version]).encode())
            atomic_write(current, job.name.encode())
            command("systemd-run", "--quiet", "--collect", "--unit=" + UNIT,
                    "--property=Restart=on-failure", "--property=RestartSec=1s",
                    "/usr/bin/python3", "-I", "-B", str(job / "bundle" / "upgrade.py"), str(job), check=True)
    print(json.dumps({"stage": "queued", "job": job.name}), flush=True)
    with (job / "events.jsonl").open("a+", encoding="utf-8") as stream:
        stream.seek(0)
        while True:
            offset = stream.tell()
            raw = stream.readline()
            if raw.endswith("\n"):
                print(raw, end="", flush=True)
                if json.loads(raw)["stage"] == "done":
                    return
                continue
            stream.seek(offset)
            time.sleep(0.5)
            if not active() and not stream.readline():
                raise ValueError("The VPS update was interrupted. Start the update again to recover.")
            stream.seek(offset)


if __name__ == "__main__":
    job = Path(sys.argv[1])
    host, version = json.loads((job / "request.json").read_text())
    with lock(APP / ".setup.lock"):
        execute(job / "bundle", job, host, version)
