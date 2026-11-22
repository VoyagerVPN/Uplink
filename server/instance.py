"""Локальный API владельца тестового инстанса Uplink."""
import ipaddress
import json
import os
import re
import socket
import socketserver
import sqlite3
import urllib.error
import urllib.request
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


def network_config(data):
    name = network_name(data.get("name"))
    try:
        subnet = ipaddress.IPv4Network(data.get("subnet", ""))
    except (ValueError, TypeError):
        raise ApiError(400, "Expected an IPv4 network")
    private = ("10.0.0.0/8", "172.16.0.0/12", "192.168.0.0/16")
    if subnet.prefixlen != 24 or not any(subnet.subnet_of(ipaddress.IPv4Network(r)) for r in private):
        raise ApiError(400, "The test instance supports private /24 networks")
    return {
        "name": name.strip(), "private": True, "enableBroadcast": True,
        "v4AssignMode": {"zt": True},
        "routes": [{"target": str(subnet), "via": None}],
        "ipAssignmentPools": [{"ipRangeStart": str(subnet[20]), "ipRangeEnd": str(subnet[239])}],
    }


def member_update(data):
    if set(data) != {"authorized"} or type(data["authorized"]) is not bool:
        raise ApiError(400, "Expected a boolean authorized field")
    return {"authorized": data["authorized"]}


def adapter_mac_prefix(network_id):
    # Эти биты ZeroTier One 1.16.2 использует в MAC::fromAddress для каждого узла сети.
    number = int(network_id, 16)
    first = (number & 0xfc) | 2
    return (0x32 if first == 0x52 else first, (number >> 8) & 0xffffffffff)


TOKEN_FILE = Path("/var/lib/uplink-zt/authtoken.secret")


class NoRedirect(urllib.request.HTTPRedirectHandler):
    def redirect_request(self, req, fp, code, msg, headers, newurl):
        return None


def controller(path, payload=None):
    request = urllib.request.Request(
        "http://127.0.0.1:9993" + path,
        headers={"X-ZT1-Auth": TOKEN_FILE.read_text().strip(), "Content-Type": "application/json"},
        data=json.dumps(payload).encode() if payload is not None else None,
    )
    client = urllib.request.build_opener(urllib.request.ProxyHandler({}), NoRedirect())
    try:
        with client.open(request, timeout=5) as response:
            return json.load(response)
    except urllib.error.HTTPError as error:
        raise ApiError(404 if error.code == 404 else 502, "Controller request failed") from error
    except (OSError, ValueError) as error:
        raise ApiError(503, "Controller unavailable") from error


def dispatch(method, path, data):
    group = re.fullmatch(r"/v1/groups/([0-9a-f]{32})", path)
    if method == "PUT" and group:
        from invites import Invitations
        return Invitations().create_group(group[1], data)
    owner = re.fullmatch(r"/v1/networks/([0-9a-f]{16}|[0-9a-f]{32})/owner", path)
    if method == "PUT" and owner and re.fullmatch("[0-9a-f]{10}", str(data.get("node", ""))):
        from invites import Invitations
        Invitations().assign_owner(owner[1], data["node"])
        return 200, {"node":data["node"]}
    invite = re.fullmatch(r"/v1/networks/([0-9a-f]{16}|[0-9a-f]{32})/invites", path)
    if method == "POST" and invite:
        from invites import Invitations
        return 201, {"token": Invitations().issue(invite[1]), "expires_in": 3600}
    if method == "GET" and path == "/v1/instance":
        return 200, {"controller": controller("/status")["address"], "networks": controller("/controller/network")}
    match = re.fullmatch(r"/v1/networks/([0-9a-f]{16})(?:/members(?:/([0-9a-f]{10}))?)?", path)
    if not match:
        raise ApiError(404, "Unknown endpoint")
    network_id, member_id = match.groups()
    if not network_id.startswith(controller("/status")["address"]):
        raise ApiError(404, "Network belongs to another instance")
    endpoint = "/controller/network/" + network_id
    if "/members" in path:
        known = controller(endpoint + "/member")
        if method == "GET" and member_id is None:
            return 200, {"members": list(known)}
        if member_id is None or member_id not in known:
            raise ApiError(404, "No membership request from this node")
        if method == "PUT":
            fields = member_update(data)
        elif method == "GET":
            fields = None
        else:
            raise ApiError(405, "Unsupported method")
        member = controller(endpoint + "/member/" + member_id, fields)
        return 200, {k: member[k] for k in ("address", "authorized", "ipAssignments")}
    if method == "GET":
        network = controller(endpoint)
        return 200, {k: network[k] for k in ("id", "name", "private", "routes", "enableBroadcast")}
    if method != "PUT":
        raise ApiError(405, "Unsupported method")
    from invites import Invitations
    import groups
    config = network_config(data)
    with Invitations().transaction() as db:
        groups.require_available_name(db, controller, network_id, config["name"])
        return create_legacy_network(endpoint, network_id, config)


def create_legacy_network(endpoint, network_id, config):
    networks = controller("/controller/network")
    for existing_id in ([network_id] if network_id in networks else networks):
        existing = controller("/controller/network/" + existing_id)
        if existing_id == network_id:
            if all(existing.get(k) == v for k, v in config.items()
                   if k not in ("v4AssignMode",)) and existing.get("v4AssignMode", {}).get("zt"):
                return 200, {"id": network_id}
            raise ApiError(409, "Network ID already in use")
        if adapter_mac_prefix(existing_id) == adapter_mac_prefix(network_id):
            raise ApiError(409, "Choose a network ID with a distinct adapter MAC")
        subnet = ipaddress.ip_network(config["routes"][0]["target"])
        if any(subnet.overlaps(ipaddress.ip_network(route["target"])) for route in existing["routes"]
               if ":" not in route["target"]):
            raise ApiError(409, "Subnet overlaps another network")
    network = controller(endpoint, config)
    return 201, {"id": network["id"]}


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
            size = int(self.headers.get("Content-Length", "0"))
            if not 0 <= size <= self.request_limit():
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
        self.end_headers()
        self.wfile.write(body)

    do_GET = do_PUT = do_POST = handle_request


def serve():
    if os.environ.get("LISTEN_PID") != str(os.getpid()) or os.environ.get("LISTEN_FDS") != "1":
        raise RuntimeError("Start through uplink-instance.socket")
    with socketserver.UnixStreamServer("", OwnerRequest, bind_and_activate=False) as server:
        server.socket.close()
        server.socket = socket.socket(fileno=3)
        server.serve_forever()


if __name__ == "__main__":
    serve()
