"""API клиентов. TLS завершается на обратном прокси инстанса."""
import hashlib
import re
import os
import time
import threading
from pathlib import Path
from contextlib import contextmanager
from collections import deque
from http.server import ThreadingHTTPServer

from instance import ApiError, OwnerRequest, networks


class LoginBudget:
    def __init__(self, attempts, window):
        self.recent = deque()
        self.attempts = attempts
        self.window = window

        self.lock = threading.Lock()
        self.slots = threading.BoundedSemaphore(2)

    @contextmanager
    def admit(self):
        with self.lock:
            now = time.monotonic()
            while self.recent and self.recent[0] <= now - self.window:
                self.recent.popleft()
            if len(self.recent) >= self.attempts or not self.slots.acquire(blocking=False):
                raise ApiError(429, "Too many login attempts")
            self.recent.append(now)
        try:
            yield
        finally:
            self.slots.release()


class ClientServer(ThreadingHTTPServer):
    def __init__(self, *args, **kwargs):
        self.workers = threading.BoundedSemaphore(16)
        super().__init__(*args, **kwargs)

    def process_request(self, request, address):
        if not self.workers.acquire(blocking=False):
            self.shutdown_request(request)
            return
        try:
            super().process_request(request, address)
        except BaseException:
            self.workers.release()
            raise

    def process_request_thread(self, request, address):
        try:
            super().process_request_thread(request, address)
        finally:
            self.workers.release()


LOGIN_BUDGET = LoginBudget(12, 60)
SERVER_REVISION = int(Path(__file__).with_name("revision.txt").read_text())
MAINTENANCE = Path(__file__).with_name(".update-in-progress")


def route(method, path, data):
    if method != "POST":
        raise ApiError(403, "Client access only")
    if path == "/v1/ping":
        return 200, {"server_revision": SERVER_REVISION}
    if MAINTENANCE.exists():
        raise ApiError(503, "The server is updating")
    api = networks()
    if path == "/v1/bootstrap":
        return 200, api.lookup(data.get("token"))
    if path == "/v1/login":
        with LOGIN_BUDGET.admit():
            return 200, {"token": api.login(data.get("name"), data.get("password"))}
    if path in ("/v1/redeem", "/v1/profile"):
        profile = (api.redeem(data.get("token"), data.get("session"), data.get("nick"))
                   if path == "/v1/redeem" else api.profile(data.get("session")))
        return 200, {**profile, "endpoint": os.environ["UPLINK_UDP_ENDPOINT"]}
    if path == "/v1/leave":
        return 200, api.leave(data.get("session"))
    if path == "/v1/ssh/request":
        return 200, api.request_ssh(data.get("session"), data.get("recipient"), data.get("public_key"))
    if path == "/v1/ssh/respond":
        return 200, api.respond_ssh(data.get("session"), data.get("id"), data.get("decision"))
    if path == "/v1/sync":
        session, network = data.get("session"), data.get("network")
        snapshot = api.sync(session, data.get("nick"), data.get("avatar_revision"), network,
                            include_ssh=data.get("ssh_requests") is True)
        snapshot["server_revision"] = SERVER_REVISION
        expiry = data.get("credential_expires_at")
        if network == snapshot["id"] and type(expiry) is int and 0 <= expiry <= api.now():
            # profile повторно проверяет членство под той же блокировкой, что и отзыв.
            snapshot["transport_profile"] = {**api.profile(session), "endpoint": os.environ["UPLINK_UDP_ENDPOINT"]}
        return 200, snapshot
    if path == "/v1/avatar":
        return 200, api.manage(data.get("session"), "avatar", data.get("value"))
    if path == "/v1/nickname":
        return 200, api.rename_member(data.get("session"), data.get("node"), data.get("nick"))
    if path == "/v1/manage":
        return 200, api.manage(data.get("session"), data.get("action"), data.get("value"))
    if path == "/v1/share":
        return 200, {"token":api.share(data.get("session"))}
    raise ApiError(403, "Client access only")


# Из журнала нельзя восстановить билет. Метка действует до перезапуска процесса.
_LOG_KEY = os.urandom(32)
_STAGES = {"/v1/" + s: s for s in (
    "login", "bootstrap", "redeem", "profile", "sync", "share", "avatar", "nickname", "manage", "ping", "leave"
)}
_STAGES.update({"/v1/ssh/request": "ssh_request", "/v1/ssh/respond": "ssh_respond"})
_REASONS = {msg: msg.lower().replace(" ", "_") for msg in (
    "Invalid network credentials", "Invalid credential", "Invalid or expired invitation",
    "Invitation already used", "Invitations are disabled", "Waiting for network join",
    "Membership revoked", "Network engine unavailable", "Invalid network engine response",
    "Session already enrolled", "Network is full",
    "Network deleted", "Invalid session", "Invalid membership",
    "Too many login attempts", "Invalid nickname", "Invalid node",
    "Invalid request", "Invalid body size", "Expected a JSON object", "Local service unavailable",
    "Client access only", "Unknown private network", "Invalid participant", "Unknown participant",
    "Invalid SSH request response", "Unknown SSH request", "SSH request already closed",
    "Invalid SSH public key", "SSH request already has a key", "The server is updating",
    "Network management permission required", "Owner permission required",
)}


class ClientRequest(OwnerRequest):
    protocol_version = "HTTP/1.1"
    def address_string(self):
        return "uplink-client"

    def response_context(self, data, status, result):
        error = result.get("error") if isinstance(result, dict) else None
        self.reason = "ok" if status < 400 else _REASONS.get(error, "request_failed")
        source = result if self.path in ("/v1/login", "/v1/share") else data
        token = source.get("token") if isinstance(source, dict) else None
        self.ticket_tag = "-"
        if self.path in ("/v1/login", "/v1/share", "/v1/bootstrap", "/v1/redeem") and isinstance(token, str) and re.fullmatch(r"[a-f0-9]{64}", token):
            self.ticket_tag = hashlib.blake2s(token.encode(), key=_LOG_KEY, digest_size=8).hexdigest()

    def log_error(self, fmt, *args):
        # BaseHTTPRequestHandler включает непроверенные метод и строку запроса.
        # send_error также вызывает log_request с безопасным числовым статусом.
        pass

    def handle_one_request(self):
        self.reason = "http_error"
        self.ticket_tag = "-"
        super().handle_one_request()

    route = staticmethod(route)

    def request_limit(self):
        return 1_411_024 if self.command == "POST" and self.path == "/v1/avatar" else super().request_limit()

    def log_request(self, code="-", size="-"):
        # При ошибке клиент может прислать секрет в пути.
        stage = _STAGES.get(getattr(self, "path", ""), "unknown")
        self.log_message("stage=%s status=%s reason=%s ticket=%s", stage, code, self.reason, self.ticket_tag)


if __name__ == "__main__":
    with ClientServer((os.environ.get("UPLINK_BIND", "127.0.0.1"), 8930), ClientRequest) as server:
        server.serve_forever()
