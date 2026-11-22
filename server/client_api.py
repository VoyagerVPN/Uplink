"""API клиентов. TLS завершается на обратном прокси инстанса."""
import base64
import os
import time
import threading
from contextlib import contextmanager
from collections import deque
from http.server import ThreadingHTTPServer
from pathlib import Path

from instance import ApiError, OwnerRequest
from invites import Invitations


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


def route(method, path, data):
    api = Invitations()
    if method != "POST":
        raise ApiError(403, "Client access only")
    if path == "/v1/bootstrap":
        info = api.lookup(data.get("token"))
        if "transport" in info and data.get("version") != 2:
            raise ApiError(400, "Update Uplink to join a logical network")
        info["planet"] = base64.b64encode(Path("/var/lib/uplink-zt/planet").read_bytes()).decode("ascii")
        return 200, info
    if path == "/v1/login":
        with LOGIN_BUDGET.admit():
            return 200, {"token": api.login(data.get("name"), data.get("password"))}
    if path == "/v1/redeem":
        return 200, api.redeem(data.get("token"), data.get("node"), data.get("session"), data.get("nick"), data.get("prior_session"), data.get("proof"))
    if path == "/v1/attest":
        return 200, api.attest(data.get("session"), data.get("proof"))
    if path == "/v1/leave":
        return 200, api.leave(data.get("session"))
    if path == "/v1/sync":
        return 200, api.sync(data.get("session"), data.get("nick"), data.get("avatar_revision"))
    if path == "/v1/avatar":
        return 200, api.manage(data.get("session"), "avatar", data.get("value"))
    if path == "/v1/manage":
        return 200, api.manage(data.get("session"), data.get("action"), data.get("value"))
    if path == "/v1/ping":
        return 200, {}
    if path == "/v1/share":
        return 200, {"token":api.share(data.get("session"))}
    raise ApiError(403, "Client access only")


class ClientRequest(OwnerRequest):
    route = staticmethod(route)

    def request_limit(self):
        return 1_411_024 if self.command == "POST" and self.path == "/v1/avatar" else super().request_limit()

    def log_request(self, code="-", size="-"):
        # При ошибке клиент может прислать секрет в пути.
        self.log_message("request status %s", code)


if __name__ == "__main__":
    with ClientServer((os.environ.get("UPLINK_BIND", "127.0.0.1"), 8930), ClientRequest) as server:
        server.serve_forever()
