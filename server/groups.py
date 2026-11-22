"""Логические сети одного инстанса, изолированные подписанными метками ZeroTier."""
import ipaddress
import re
import secrets
import unicodedata

from instance import ApiError, adapter_mac_prefix, network_config, network_name


def schema(db):
    db.execute("CREATE TABLE IF NOT EXISTS shared_transport (slot INTEGER PRIMARY KEY CHECK(slot=1), id TEXT UNIQUE, subnet TEXT)")
    db.execute("CREATE TABLE IF NOT EXISTS groups (tag INTEGER PRIMARY KEY, id TEXT UNIQUE, name TEXT NOT NULL)")
    db.execute("CREATE TABLE IF NOT EXISTS departed (session TEXT PRIMARY KEY)")


def name_key(name):
    return unicodedata.normalize("NFC", name.strip().casefold())


def require_available_name(db, backend, network, name):
    key = name_key(name)
    names = db.execute("SELECT id,name FROM groups WHERE id<>?", (network,))
    if any(name_key(row["name"]) == key for row in names):
        raise ApiError(409, "Network name already in use")
    shared = db.execute("SELECT id FROM shared_transport").fetchone()
    for existing in backend("/controller/network"):
        if existing != network and (not shared or existing != shared["id"]):
            info = backend("/controller/network/"+existing)
            if name_key(info["name"]) == key:
                raise ApiError(409, "Network name already in use")


def select_transport(controller, existing):
    used = {adapter_mac_prefix(network) for network in existing}
    for _ in range(64):
        transport = controller + secrets.token_hex(3)
        if adapter_mac_prefix(transport) not in used:
            break
    else:
        raise ApiError(409, "Specify a free transport ID")

    # Начинаем с произвольной /24, пересекающиеся маршруты пропускаем целиком.
    pool = ipaddress.IPv4Network("10.0.0.0/8")
    start = int(pool.network_address) + (secrets.randbelow(65536) << 8)
    routes = sorted(ipaddress.IPv4Network(r["target"]) for config in existing.values()
                    for r in config["routes"] if ":" not in r["target"])
    for cursor, end in ((start, int(pool.broadcast_address)), (int(pool.network_address), start - 1)):
        for route in routes:
            if cursor + 255 < int(route.network_address):
                break
            if cursor <= int(route.broadcast_address):
                cursor = (int(route.broadcast_address) | 255) + 1
        if cursor + 255 <= end:
            return transport, str(ipaddress.IPv4Network((cursor, 24)))
    raise ApiError(409, "Specify an available private /24 subnet")


def create(api, group, data):
    if not re.fullmatch("[a-f0-9]{32}", group):
        raise ApiError(400, "Invalid group ID")
    name = network_name(data.get("name"))
    controller = api.transport("/status")["address"]
    # Резервируем пустой транспорт до обращения на запись. Повтор после сбоя безопасен.
    with api.transaction() as db:
        current = db.execute("SELECT id,subnet FROM shared_transport").fetchone()
        existing_networks = {} if current else {
            network: api.transport("/controller/network/" + network)
            for network in api.transport("/controller/network")
        }
        if "transport" not in data and "subnet" not in data:
            transport, subnet = tuple(current) if current else select_transport(controller, existing_networks)
        else:
            transport, subnet = data.get("transport"), data.get("subnet")
        config = network_config({"name": name, "subnet": subnet})
        subnet = config["routes"][0]["target"]
        if not isinstance(transport, str) or not re.fullmatch("[a-f0-9]{16}", transport):
            raise ApiError(400, "Invalid identifier")
        if not transport.startswith(controller):
            raise ApiError(400, "Transport belongs to another controller")
        if current and tuple(current) != (transport, subnet):
            raise ApiError(409, "Instance already has a different shared transport")
        if not current:
            require_available_name(db, api.transport, group, config["name"])
            for existing, existing_config in existing_networks.items():
                if adapter_mac_prefix(existing) == adapter_mac_prefix(transport):
                    raise ApiError(409, "Transport ID already in use")
                routes = existing_config["routes"]
                if any(ipaddress.ip_network(subnet).overlaps(ipaddress.ip_network(route["target"]))
                       for route in routes if ":" not in route["target"]):
                    raise ApiError(409, "Transport subnet overlaps another network")
            db.execute("INSERT INTO shared_transport VALUES(1,?,?)", (transport, subnet))
    with api.transaction() as db:
        existing = db.execute("SELECT name FROM groups WHERE id=?", (group,)).fetchone()
        if existing and existing["name"] != config["name"]:
            raise ApiError(409, "Group already exists with another name")
        if not existing:
            require_available_name(db,api.transport,group,config["name"])
            if db.execute("SELECT count(*) FROM groups").fetchone()[0] >= 128:
                raise ApiError(409, "ZeroTier network tag limit reached")
            db.execute("INSERT INTO groups(id,name) VALUES(?,?)", (group, config["name"]))
        tags = [r[0] for r in db.execute("SELECT tag FROM groups ORDER BY tag")]
        rules = []
        for tag in tags:
            rules.extend(({"type":"MATCH_TAGS_EQUAL","id":tag,"value":1}, {"type":"ACTION_ACCEPT"}))
        config.update(name="Uplink shared transport", tags=[{"id":tag,"default":0} for tag in tags],
                      rules=[*rules, {"type":"ACTION_DROP"}])
        api.transport("/controller/network/"+transport, config)
        return (200 if existing else 201), {"id":group, "transport":transport}


def group_info(db, network):
    return db.execute("SELECT groups.*,shared_transport.id AS transport FROM groups CROSS JOIN shared_transport WHERE groups.id=?",
                      (network,)).fetchone()


def directory(db, backend, path, payload=None):
    prefix = "/controller/network/"
    network, _, tail = path.removeprefix(prefix).partition("/")
    group = group_info(db, network)
    if not group:
        if db.execute("SELECT 1 FROM shared_transport WHERE id=?", (network,)).fetchone():
            raise ApiError(403, "Transport is managed through logical networks")
        return backend(path) if payload is None else backend(path, payload)
    if not tail:
        if payload is not None:
            db.execute("UPDATE groups SET name=? WHERE id=?", (payload["name"],network))
        return {"id":network, "name":group["name"], "private":True, "transport":group["transport"]}
    endpoint = prefix + group["transport"] + "/" + tail
    if tail == "member":
        return backend(endpoint)
    member = backend(endpoint)
    tags = dict(member.get("tags", []))
    if payload is not None:
        granting = payload["authorized"]
        tags[group["tag"]] = int(granting)
        member = backend(endpoint, {"tags":sorted(tags.items()), "authorized":granting or
                                    (member["authorized"] and any(tags.values()))})
    return {**member, "authorized":bool(member["authorized"] and dict(member.get("tags", [])).get(group["tag"]) == 1)}

