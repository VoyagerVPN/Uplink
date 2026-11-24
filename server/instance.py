"""Локальный API владельца тестового инстанса Uplink."""
import json
import os
import re
import socket
import socketserver
import sqlite3
from http.server import BaseHTTPRequestHandler
from pathlib import Path

class ApiError(Exception):
    def __init__(self, status, message):
        super().__init__(message)
        self.status = status


def network_name(value):
    if not isinstance(value, str) or not value.isprintable() or not 1 <= len(value.strip()) <= 64:
        raise ApiError(400, "Expected a network name (1 to 64 characters)")
    return value.strip()


def networks():
    from membership import Networks
    from easytier import Credentials
    return Networks(Path("/var/lib/uplink-members/networks.db"), Credentials())


def dispatch(method, path, data):
    api = networks()
    if method == "GET" and path == "/v1/instance":
        with api.transaction() as db:
            return 200, {"engine":"easytier","networks":[r[0] for r in db.execute("SELECT id FROM networks")]}
    group = re.fullmatch(r"/v1/groups/([0-9a-f]{32})", path)
    if method == "PUT" and group:
        return api.create_group(group[1], data)
    target = re.fullmatch(r"/v1/networks/([0-9a-f]{32})/(owner|invites)", path)
    if target and method == "POST" and target[2] == "invites":
        return 201, {"token":api.issue(target[1]), "expires_in":3600}
    if target and method == "PUT" and target[2] == "owner":
        api.assign_owner(target[1], data.get("node"))
        return 200, {}
    raise ApiError(404, "Unknown endpoint")


class OwnerRequest(BaseHTTPRequestHandler):
    timeout = 8
    route = staticmethod(dispatch)

    def address_string(self):
        return "local-owner"

    def request_limit(self):
        return 4096

    def response_context(self, data, status, result):
        pass

    def handle_request(self):
        data = {}
        try:
            sizes = self.headers.get_all("Content-Length", [])
            if self.headers.get("Transfer-Encoding") or len(sizes) > 1 or (sizes and not re.fullmatch(r"[0-9]{1,10}", sizes[0])):
                self.close_connection = True
                raise ApiError(400, "Invalid body size")
            size = int(sizes[0]) if sizes else 0
            if not 0 <= size <= self.request_limit():
                self.close_connection = True
                raise ApiError(413, "Invalid body size")
            data = json.loads(self.rfile.read(size)) if size else {}
            if not isinstance(data, dict):
                raise ApiError(400, "Expected a JSON object")
            status, result = self.route(self.command, self.path, data)
        except ApiError as error:
            status, result = error.status, {"error": str(error)}
        except (ValueError, UnicodeError):
            status, result = 400, {"error": "Invalid request"}
        except (OSError, sqlite3.OperationalError):
            status, result = 503, {"error": "Local service unavailable"}
        body = json.dumps(result).encode()
        self.response_context(data, status, result)
        self.send_response(status)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(body)))
        if self.close_connection:
            self.send_header("Connection", "close")
        self.end_headers()
        self.wfile.write(body)

    do_GET = do_PUT = do_POST = handle_request


def serve():
    if os.environ.get("LISTEN_PID") != str(os.getpid()) or os.environ.get("LISTEN_FDS") != "1":
        raise RuntimeError("Start through uplink-owner.socket")
    with socketserver.UnixStreamServer("", OwnerRequest, bind_and_activate=False) as server:
        server.socket.close()
        server.socket = socket.socket(fileno=3)
        server.serve_forever()


if __name__ == "__main__":
    serve()
