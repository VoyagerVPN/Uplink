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

def ssh_public_key(value):
    if not isinstance(value, str) or not re.fullmatch(r"ssh-ed25519 [A-Za-z0-9+/]{68}", value):
        raise ApiError(400, "Invalid SSH public key")
    encoded = value.split(" ")[1]
    wire = base64.b64decode(encoded, validate=True)
    if len(wire) != 51 or not wire.startswith(b"\0\0\0\x0bssh-ed25519\0\0\0\x20"):
        raise ApiError(400, "Invalid SSH public key")
    return value


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
            db.execute("""CREATE TABLE IF NOT EXISTS aliases (
                session TEXT PRIMARY KEY REFERENCES members(session) ON DELETE CASCADE, nick TEXT NOT NULL)""")
            db.execute("CREATE TABLE IF NOT EXISTS departed (session TEXT PRIMARY KEY)")
            db.execute("""CREATE TABLE IF NOT EXISTS ssh_requests (
                id TEXT PRIMARY KEY,
                requester TEXT NOT NULL REFERENCES members(session) ON DELETE CASCADE,
                recipient TEXT NOT NULL REFERENCES members(session) ON DELETE CASCADE,
                state TEXT NOT NULL DEFAULT 'pending' CHECK(state IN ('pending','rejected','cancelled')),
                requested_at INTEGER NOT NULL DEFAULT 0,
                public_key TEXT,
                UNIQUE(requester, recipient))""")
            ssh_columns = {column["name"] for column in db.execute("PRAGMA table_info(ssh_requests)")}
            if "requested_at" not in ssh_columns:
                db.execute("ALTER TABLE ssh_requests ADD COLUMN requested_at INTEGER NOT NULL DEFAULT 0")
            if "public_key" not in ssh_columns:
                db.execute("ALTER TABLE ssh_requests ADD COLUMN public_key TEXT")
            db.execute("CREATE INDEX IF NOT EXISTS ssh_request_recipient ON ssh_requests(recipient)")
            db.execute("""CREATE TABLE IF NOT EXISTS ssh_history (
                id TEXT PRIMARY KEY,
                requester TEXT NOT NULL REFERENCES members(session) ON DELETE CASCADE,
                recipient TEXT NOT NULL REFERENCES members(session) ON DELETE CASCADE,
                state TEXT NOT NULL CHECK(state IN ('rejected','cancelled')),
                occurred_at INTEGER NOT NULL)""")
            db.execute("CREATE INDEX IF NOT EXISTS ssh_history_requester ON ssh_history(requester, occurred_at)")
            db.execute("CREATE INDEX IF NOT EXISTS ssh_history_recipient ON ssh_history(recipient, occurred_at)")
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

    def request_ssh(self, session, recipient, public_key=None):
        key = digest(session)
        if public_key is not None:
            public_key = ssh_public_key(public_key)
        if not isinstance(recipient, str) or not re.fullmatch("[a-f0-9]{10}", recipient):
            raise ApiError(400, "Invalid participant")
        with self.transaction() as db:
            sender = db.execute("SELECT node, network FROM members WHERE session=? AND removing=0", (key,)).fetchone()
            if not sender:
                raise ApiError(403, "Invalid session")
            if sender["node"] == recipient:
                raise ApiError(400, "Invalid participant")
            target = db.execute("SELECT session FROM members WHERE network=? AND node=? AND removing=0",
                                (sender["network"], recipient)).fetchone()
            if not target:
                raise ApiError(404, "Unknown participant")
            pair = (key, target["session"])
            db.execute("""INSERT INTO ssh_requests(id, requester, recipient, requested_at, public_key) VALUES(?,?,?,?,?)
                          ON CONFLICT(requester, recipient) DO UPDATE SET id=excluded.id, state='pending',
                          requested_at=excluded.requested_at, public_key=excluded.public_key
                          WHERE ssh_requests.state!='pending'
                             OR (ssh_requests.public_key IS NULL AND excluded.public_key IS NOT NULL)""",
                       (secrets.token_hex(16), *pair, int(self.now()), public_key))
            request = db.execute("SELECT id, requested_at, public_key FROM ssh_requests WHERE requester=? AND recipient=?", pair).fetchone()
            if request["public_key"] != public_key:
                raise ApiError(409, "SSH request already has a key")
            reply = {"id": request["id"], "requester": sender["node"], "recipient": recipient,
                     "state": "pending", "occurred_at": request["requested_at"]}
            if public_key is not None:
                reply["public_key"] = public_key
            return reply

    def respond_ssh(self, session, request_id, decision):
        key = digest(session)
        if (not isinstance(request_id, str) or not re.fullmatch("[a-f0-9]{32}", request_id)
                or decision not in ("cancel", "reject")):
            raise ApiError(400, "Invalid SSH request response")
        party, next_state = ("requester_key", "cancelled") if decision == "cancel" else ("recipient_key", "rejected")
        with self.transaction() as db:
            request = db.execute("""
                SELECT r.state, r.requested_at, r.public_key, r.requester AS requester_key, r.recipient AS recipient_key,
                       sender.node AS requester, target.node AS recipient
                FROM ssh_requests r JOIN members sender ON sender.session=r.requester
                JOIN members target ON target.session=r.recipient
                WHERE r.id=? AND sender.removing=0 AND target.removing=0""", (request_id,)).fetchone()
            if not request or request[party] != key:
                raise ApiError(404, "Unknown SSH request")
            if request["state"] not in ("pending", next_state):
                raise ApiError(409, "SSH request already closed")
            db.execute("UPDATE ssh_requests SET state=? WHERE id=?", (next_state, request_id))
            db.execute("""INSERT OR IGNORE INTO ssh_history(id,requester,recipient,state,occurred_at)
                          VALUES(?,?,?,?,?)""",
                       (request_id, request["requester_key"], request["recipient_key"], next_state, int(self.now())))
            reply = {"id": request_id, "requester": request["requester"],
                     "recipient": request["recipient"], "state": next_state, "occurred_at": request["requested_at"]}
            if request["public_key"] is not None:
                reply["public_key"] = request["public_key"]
            return reply

    def sync(self, session, nick, avatar_revision=None, network=None, *, include_ssh=False):
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
                        "nick": m["display_nick"], "role": m["role"], "present": m["seen"] > now - 45}
                       for m in db.execute("""SELECT m.*, COALESCE(a.nick, m.nick) AS display_nick
                           FROM members m LEFT JOIN aliases a ON a.session=m.session
                           WHERE m.network=? AND m.removing=0 ORDER BY m.address""", (row["id"],))]
            methods = {"link": bool(row["link"]), "qr": bool(row["qr"]), "password": bool(row["verifier"])}
            if row["salt"]:
                methods["password_revision"] = hashlib.sha256(row["salt"]).hexdigest()
            result = {"id": row["id"], "name": row["name"], "members": members, "methods": methods}
            if include_ssh:
                result["ssh_key_requests"] = True
                result["ssh_requests"] = [{k: value for k, value in dict(request).items() if value is not None} for request in db.execute("""
                    SELECT r.id, sender.node AS requester, target.node AS recipient, r.state, r.requested_at AS occurred_at, r.public_key
                    FROM ssh_requests r JOIN members sender ON sender.session=r.requester
                    JOIN members target ON target.session=r.recipient
                    WHERE (r.requester=? OR r.recipient=?) AND sender.removing=0 AND target.removing=0
                    ORDER BY r.requested_at DESC, r.rowid DESC""", (key, key))]
                result["ssh_history"] = [dict(event) for event in db.execute("""
                    SELECT h.id, sender.node AS requester, target.node AS recipient, h.state, h.occurred_at
                    FROM ssh_history h JOIN members sender ON sender.session=h.requester
                    JOIN members target ON target.session=h.recipient
                    WHERE h.requester=? OR h.recipient=?
                    ORDER BY h.occurred_at DESC, h.rowid DESC LIMIT 100""", (key, key))]
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
            member = db.execute("SELECT network FROM members WHERE session=? AND removing=0",
                                (digest(session),)).fetchone()
            if not member:
                raise ApiError(403, "Invalid session")
            group = member["network"]
            if not db.execute("SELECT 1 FROM networks WHERE id=? AND (link OR qr)", (group,)).fetchone():
                raise ApiError(403, "Invitations are disabled")
            return self.mint(db, group, "share")

    def rename_member(self, session, node, name):
        key, name = digest(session), nickname(name)
        if not isinstance(node, str) or not re.fullmatch("[a-f0-9]{10}", node):
            raise ApiError(400, "Invalid participant")
        with self.transaction() as db:
            actor = db.execute("SELECT * FROM members WHERE session=? AND removing=0", (key,)).fetchone()
            if not actor:
                raise ApiError(403, "Invalid session")
            target = db.execute("SELECT * FROM members WHERE network=? AND node=? AND removing=0",
                                (actor["network"], node)).fetchone()
            if not target:
                raise ApiError(404, "Unknown participant")
            if actor["session"] != target["session"] and not (
                actor["role"] == "owner" and target["role"] != "owner" or
                actor["role"] == "admin" and target["role"] == "member"
            ):
                raise ApiError(403, "Network management permission required")
            db.execute("""INSERT INTO aliases(session,nick) VALUES (?,?)
                          ON CONFLICT(session) DO UPDATE SET nick=excluded.nick""", (target["session"], name))
        return {}

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
