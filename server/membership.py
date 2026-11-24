"""Сети Uplink и персональные ключи участников EasyTier."""
import base64
import hashlib
import ipaddress
import re
import secrets
import sqlite3
import time
import zlib
import unicodedata
from contextlib import closing, contextmanager
from pathlib import Path

from instance import ApiError, network_name
def digest(value):
    if not isinstance(value, str) or not re.fullmatch("[a-f0-9]{64}", value):
        raise ApiError(403, "Invalid credential")
    return hashlib.sha256(value.encode("ascii")).hexdigest()


def password_hash(password, salt):
    if not isinstance(password,str) or not 1 <= len(password.encode("utf-8")) <= 512:
        raise ApiError(400, "Invalid network password")
    return hashlib.scrypt(password.encode("utf-8"),salt=salt,n=2**15,r=8,p=1,maxmem=64*1024*1024,dklen=32)


def nickname(value):
    if not isinstance(value, str) or not 1 <= len(value.strip()) <= 32 or not value.isprintable():
        raise ApiError(400, "Invalid nickname")
    return value.strip()



def name_key(name):
    return unicodedata.normalize("NFC", name.strip().casefold())


class Networks:
    def __init__(self, path, engine, now=time.time):
        self.path = Path(path)
        self.engine = engine
        self.now = now

    @contextmanager
    def transaction(self):
        with closing(sqlite3.connect(self.path, timeout=8)) as db, db:
            db.row_factory = sqlite3.Row
            db.execute("PRAGMA foreign_keys=ON")
            db.execute("BEGIN IMMEDIATE")
            db.execute("""CREATE TABLE IF NOT EXISTS networks (
                id TEXT PRIMARY KEY, name TEXT NOT NULL, name_key TEXT UNIQUE NOT NULL,
                subnet INTEGER UNIQUE NOT NULL, link INTEGER DEFAULT 1, qr INTEGER DEFAULT 0,
                salt BLOB, verifier BLOB, avatar BLOB, revision TEXT DEFAULT '')""")
            db.execute("""CREATE TABLE IF NOT EXISTS tickets (
                token TEXT PRIMARY KEY, network TEXT REFERENCES networks(id), source TEXT NOT NULL,
                expires INTEGER NOT NULL, session TEXT UNIQUE)""")
            db.execute("""CREATE TABLE IF NOT EXISTS members (
                session TEXT PRIMARY KEY, network TEXT REFERENCES networks(id), node TEXT NOT NULL,
                address INTEGER NOT NULL, nick TEXT NOT NULL, seen INTEGER NOT NULL,
                role TEXT DEFAULT 'member', removing INTEGER DEFAULT 0, UNIQUE(network,node), UNIQUE(network,address))""")
            db.execute("CREATE UNIQUE INDEX IF NOT EXISTS sole_owner ON members(network) WHERE role='owner'")
            db.execute("CREATE TABLE IF NOT EXISTS departed (session TEXT PRIMARY KEY)")
            yield db

    def create_group(self, group, data):
        if not isinstance(group, str) or not re.fullmatch('[0-9a-f]{32}', group):
            raise ApiError(400, "Invalid group ID")
        name = network_name(data.get("name"))
        with self.transaction() as db:
            row = db.execute("SELECT name FROM networks WHERE id=?", (group,)).fetchone()
            if row:
                if row["name"] != name:
                    raise ApiError(409, "Group already exists with another name")
                return 200, {"id": group}
            if db.execute("SELECT 1 FROM networks WHERE name_key=?", (name_key(name),)).fetchone():
                raise ApiError(409, "Network name already in use")
            used = {r[0] for r in db.execute("SELECT subnet FROM networks")}
            start = secrets.randbelow(65536)
            for offset in range(65536):
                subnet = 0x0a000000 + (((start + offset) % 65536) << 8)
                if subnet not in used:
                    break
            else:
                raise ApiError(409, "No available subnet")
            db.execute("INSERT INTO networks(id,name,name_key,subnet) VALUES(?,?,?,?)",
                       (group, name, name_key(name), subnet))
            return 201, {"id": group}

    def mint(self, db, group, source):
        token = secrets.token_hex(32)
        db.execute("DELETE FROM tickets WHERE expires<=?", (self.now(),))
        db.execute("INSERT INTO tickets VALUES(?,?,?,?,NULL)",
                   (digest(token), group, source, int(self.now()) + 3600))
        return token

    def issue(self, group):
        with self.transaction() as db:
            if not db.execute("SELECT 1 FROM networks WHERE id=?", (group,)).fetchone():
                raise ApiError(404, "Unknown private network")
            return self.mint(db, group, "owner")

    def ticket(self, db, token):
        row = db.execute("""SELECT t.*,n.subnet,n.name FROM tickets t JOIN networks n
                            ON n.id=t.network WHERE t.token=? AND t.expires>?""",
                         (digest(token), self.now())).fetchone()
        if not row:
            raise ApiError(403, "Invalid or expired invitation")
        return row

    def lookup(self, token):
        with self.transaction() as db:
            ticket = self.ticket(db, token)
            return {"id": ticket["network"], "name": ticket["name"], "engine": "easytier"}

    def redeem(self, token, session, nick):
        key, nick = digest(session), nickname(nick)
        # Сначала сохраняем привязку билета. При потере ответа тот же клиент повторит выдачу.
        with self.transaction() as db:
            ticket = self.ticket(db, token)
            network = ticket["network"]
            if ticket["session"] not in (None, key):
                raise ApiError(403, "Invitation already used")
            if db.execute("SELECT 1 FROM departed WHERE session=?", (key,)).fetchone():
                raise ApiError(403, "Membership revoked")
            member = db.execute("SELECT * FROM members WHERE session=?", (key,)).fetchone()
            if not member:
                occupied = {r[0] for r in db.execute("SELECT address FROM members WHERE network=?", (network,))}
                address = next((ticket["subnet"] + i for i in range(2,255) if ticket["subnet"] + i not in occupied), None)
                if address is None:
                    raise ApiError(409, "Network is full")
                node = secrets.token_hex(5)
                db.execute("INSERT INTO members(session,network,node,address,nick,seen) VALUES(?,?,?,?,?,?)",
                           (key, network, node, address, nick, int(self.now())))
            elif member["network"] != network or ticket["session"] != key:
                raise ApiError(403, "Session already enrolled")
            db.execute("UPDATE tickets SET session=? WHERE token=?", (key, ticket["token"]))
        return self.profile(session)

    @contextmanager
    def key_lock(self, key):
        # Фиксированное число файлов ограничивает размер каталога; ядро снимает блокировку при сбое.
        path = self.path.with_name(self.path.name + ".key-" + key[:2])
        with path.open("a+b") as lock:
            import os
            if os.name == "nt":
                import msvcrt
                lock.seek(0)
                msvcrt.locking(lock.fileno(), msvcrt.LK_LOCK, 1)
            else:
                import fcntl
                fcntl.flock(lock, fcntl.LOCK_EX)
            yield

    def profile(self, session):
        key = digest(session)
        with self.key_lock(key):
            with self.transaction() as db:
                row = db.execute("""SELECT m.*,n.name FROM members m JOIN networks n ON n.id=m.network
                                    WHERE m.session=? AND m.removing=0""", (key,)).fetchone()
                if not row:
                    raise ApiError(403, "Invalid session")
            secret, expires_at = self.engine.issue(row["network"], row["node"])
            return {"id": row["network"], "node": row["node"], "name": row["name"],
                    "ip": str(ipaddress.IPv4Address(row["address"])), "credential": secret,
                    "credential_expires_at": expires_at}

    def leave(self, session):
        key = digest(session)
        return self.remove_key(key)

    def remove_key(self, key):
        with self.key_lock(key):
            with self.transaction() as db:
                if db.execute("SELECT 1 FROM departed WHERE session=?", (key,)).fetchone():
                    return {}
                row = db.execute("SELECT * FROM members WHERE session=?", (key,)).fetchone()
                if not row:
                    raise ApiError(403, "Invalid membership")
                db.execute("UPDATE members SET removing=1 WHERE session=?", (key,))
            self.engine.revoke(row["network"], row["node"])
            with self.transaction() as db:
                db.execute("INSERT INTO departed VALUES(?)", (key,))
                db.execute("DELETE FROM tickets WHERE session=?", (key,))
                db.execute("DELETE FROM members WHERE session=?", (key,))
        return {}

    def sync(self, session, nick, avatar_revision=None, network=None):
        key, nick = digest(session), nickname(nick)
        with self.transaction() as db:
            row = db.execute("""SELECT n.* FROM members m JOIN networks n ON m.network=n.id
                                WHERE m.session=? AND m.removing=0""", (key,)).fetchone()
            if not row:
                if network and not db.execute("SELECT 1 FROM networks WHERE id=?", (network,)).fetchone():
                    raise ApiError(410, "Network deleted")
                raise ApiError(403, "Invalid session")
            now = int(self.now())
            db.execute("UPDATE members SET nick=?,seen=? WHERE session=?", (nick, now, key))
            members = [{"node": m["node"], "ip": str(ipaddress.IPv4Address(m["address"])),
                        "nick": m["nick"], "role": m["role"], "present": m["seen"] > now - 45}
                       for m in db.execute("SELECT * FROM members WHERE network=? AND removing=0 ORDER BY address", (row["id"],))]
            methods = {"link": bool(row["link"]), "qr": bool(row["qr"]), "password": bool(row["verifier"])}
            if row["salt"]:
                methods["password_revision"] = hashlib.sha256(row["salt"]).hexdigest()
            result = {"id": row["id"], "name": row["name"], "members": members, "methods": methods}
            if isinstance(avatar_revision, str):
                result["avatar_revision"] = row["revision"]
                if row["avatar"] and row["revision"] != avatar_revision:
                    result["avatar"] = base64.b64encode(row["avatar"]).decode("ascii")
            return result


    def assign_owner(self, network, node):
        with self.transaction() as db:
            current = db.execute("SELECT node FROM members WHERE network=? AND role='owner'", (network,)).fetchone()
            if current and current["node"] != node:
                raise ApiError(409, "Owner already assigned")
            if not db.execute("UPDATE members SET role='owner' WHERE network=? AND node=? AND removing=0",
                              (network, node)).rowcount:
                raise ApiError(404, "Participant has no profile")

    def manager(self, db, session):
        row = db.execute("SELECT * FROM members WHERE session=? AND removing=0", (digest(session),)).fetchone()
        if not row or row["role"] not in ("owner", "admin"):
            raise ApiError(403, "Network management permission required")
        return row

    def share(self, session):
        with self.transaction() as db:
            group = self.manager(db, session)["network"]
            if not db.execute("SELECT 1 FROM networks WHERE id=? AND (link OR qr)", (group,)).fetchone():
                raise ApiError(403, "Invitations are disabled")
            return self.mint(db, group, "share")

    def login(self, name, password):
        login = name_key(network_name(name))
        with self.transaction() as db:
            row = db.execute("SELECT id,salt,verifier FROM networks WHERE name_key=? AND salt IS NOT NULL",
                             (login,)).fetchone()
        password_digest = password_hash(password, row["salt"] if row else bytes(16))
        if not row or not secrets.compare_digest(password_digest, row["verifier"]):
            raise ApiError(403, "Invalid network credentials")
        with self.transaction() as db:
            if not db.execute("SELECT 1 FROM networks WHERE name_key=? AND id=? AND verifier=?",
                              (login, row["id"], password_digest)).fetchone():
                raise ApiError(403, "Invalid network credentials")
            return self.mint(db, row["id"], "password")

    def manage(self, session, action, value):
        remove = None
        with self.transaction() as db:
            actor = self.manager(db, session)
            group = actor["network"]
            if action == "name":
                name = network_name(value)
                try:
                    db.execute("UPDATE networks SET name=?,name_key=? WHERE id=?", (name, name_key(name), group))
                except sqlite3.IntegrityError:
                    raise ApiError(409, "Network name already in use") from None
            elif action == "password" or action == "password-mode" and value == "off":
                salt = secrets.token_bytes(16) if action == "password" else None
                verifier = password_hash(value, salt) if salt else None
                db.execute("UPDATE networks SET salt=?,verifier=? WHERE id=?", (salt, verifier, group))
                db.execute("DELETE FROM tickets WHERE source='password' AND session IS NULL AND network=?", (group,))
                if salt:
                    return {"id": group, "password_revision": hashlib.sha256(salt).hexdigest()}
            elif action in ("link", "qr") and value in ("on", "off"):
                db.execute(f"UPDATE networks SET {action}=? WHERE id=?", (int(value == "on"), group))
                if not db.execute("SELECT 1 FROM networks WHERE id=? AND (link OR qr)", (group,)).fetchone():
                    db.execute("DELETE FROM tickets WHERE source='share' AND session IS NULL AND network=?", (group,))
            elif action == "avatar":
                if not isinstance(value, str) or len(value) > 1410000:
                    raise ApiError(400, "Invalid network avatar")
                try:
                    packed = base64.b64decode(value, validate=True)
                    decoder = zlib.decompressobj()
                    pixels = decoder.decompress(packed, 512*512*4+1)
                    if len(pixels) != 512*512*4 or not decoder.eof or decoder.unused_data:
                        raise ValueError()
                except (ValueError, zlib.error):
                    raise ApiError(400, "Invalid network avatar") from None
                db.execute("UPDATE networks SET avatar=?,revision=? WHERE id=?",
                           (packed, hashlib.sha256(packed).hexdigest(), group))
            elif action in ("role", "transfer", "remove"):
                if not isinstance(value,str) or not re.fullmatch("[a-f0-9]{10}", value):
                    raise ApiError(400, "Invalid participant")
                member = db.execute("SELECT * FROM members WHERE network=? AND node=?", (group,value)).fetchone()
                if not member:
                    raise ApiError(404, "Unknown participant")
                if value == actor["node"] or member["role"] == "owner":
                    raise ApiError(403, "Owner permission required")
                if actor["role"] != "owner" and (action != "remove" or member["role"] != "member"):
                    raise ApiError(403, "Owner permission required")
                if action == "remove":
                    remove = member["session"]
                    db.execute("UPDATE members SET removing=1 WHERE session=?", (remove,))
                else:
                    if member["removing"]:
                        raise ApiError(403, "Membership revoked")
                    if action == "transfer":
                        db.execute("UPDATE members SET role='member' WHERE session=?", (actor["session"],))
                    role = "owner" if action == "transfer" else ("member" if member["role"] == "admin" else "admin")
                    db.execute("UPDATE members SET role=? WHERE session=?", (role, member["session"]))
            else:
                raise ApiError(400, "Unsupported management action")
        if remove:
            self.remove_key(remove)
        return {"id": group}
