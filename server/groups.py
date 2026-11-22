"""Логические сети одного инстанса, изолированные подписанными метками ZeroTier."""
import ipaddress
import re
import unicodedata

from instance import ApiError, adapter_mac_prefix, network_config


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


def create(api, group, data):
    transport = data.get("transport")
    config = network_config(data)
    if not re.fullmatch("[a-f0-9]{32}", group) or not isinstance(transport, str) or not re.fullmatch("[a-f0-9]{16}", transport):
        raise ApiError(400, "Invalid identifier")
    if not transport.startswith(api.transport("/status")["address"]):
        raise ApiError(400, "Transport belongs to another controller")
    subnet = config["routes"][0]["target"]
    # Резервируем пустой транспорт до обращения на запись. Повтор после сбоя безопасен.
    with api.transaction() as db:
        current = db.execute("SELECT id,subnet FROM shared_transport").fetchone()
        if current and tuple(current) != (transport, subnet):
            raise ApiError(409, "Instance already has a different shared transport")
        if not current:
            require_available_name(db, api.transport, group, config["name"])
            for existing in api.transport("/controller/network"):
                if adapter_mac_prefix(existing) == adapter_mac_prefix(transport):
                    raise ApiError(409, "Transport ID already in use")
                routes = api.transport("/controller/network/"+existing)["routes"]
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

