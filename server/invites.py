"""Одноразовые приглашения и сессии отдельных участников."""
import hashlib
import re
import secrets
import sqlite3
import time
from contextlib import closing, contextmanager
from pathlib import Path

from instance import ApiError, controller, network_name
import groups

STATE = Path("/var/lib/uplink-instance/members.sqlite3")


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


class Invitations:
    def __init__(self, path=STATE, transport=controller, now=time.time, verify_device=None):
        self.path = path
        self.transport = transport
        self.now = now
        if verify_device is None:
            from device_proof import verify
            verify_device = verify
        self.verify_device = verify_device

    @contextmanager
    def transaction(self):
        with closing(sqlite3.connect(self.path, timeout=8)) as db, db:
            db.row_factory = sqlite3.Row
            db.execute("""CREATE TABLE IF NOT EXISTS invitations (
                token TEXT PRIMARY KEY, network TEXT NOT NULL, expires INTEGER NOT NULL,
                node TEXT, session TEXT UNIQUE, nick TEXT, seen INTEGER, redeemed INTEGER DEFAULT 0,
                UNIQUE(network, node))""")
            db.execute("BEGIN IMMEDIATE")
            db.execute("""CREATE TABLE IF NOT EXISTS roles (
                network TEXT, node TEXT, role TEXT NOT NULL, PRIMARY KEY(network,node))""")
            db.execute("CREATE UNIQUE INDEX IF NOT EXISTS network_owner ON roles(network) WHERE role='owner'")
            columns = {r[1] for r in db.execute("PRAGMA table_info(invitations)")}
            if "device_verified" not in columns:
                db.execute("ALTER TABLE invitations ADD COLUMN device_verified INTEGER NOT NULL DEFAULT 0")
            if "source" not in columns:
                db.execute("ALTER TABLE invitations ADD COLUMN source TEXT NOT NULL DEFAULT 'owner'")
            db.execute("""CREATE TABLE IF NOT EXISTS join_methods (
                network TEXT PRIMARY KEY, link INTEGER NOT NULL DEFAULT 1, qr INTEGER NOT NULL DEFAULT 0)""")
            db.execute("""CREATE TABLE IF NOT EXISTS network_passwords (
                network TEXT PRIMARY KEY, login TEXT NOT NULL UNIQUE, salt BLOB NOT NULL, verifier BLOB NOT NULL)""")
            groups.schema(db)
            yield db

    def directory(self, db, path, payload=None):
        return groups.directory(db, self.transport, path, payload)

    def create_group(self, group, data):
        return groups.create(self, group, data)

    def issue(self, network):
        with self.transaction() as db:
            info = self.directory(db, "/controller/network/" + network)
            if info.get("private") is not True or info.get("id") != network:
                raise ApiError(404, "Unknown private network")
            return self.mint(db,network,"owner")

    def mint(self, db, network, source):
        token = secrets.token_hex(32)
        db.execute("DELETE FROM invitations WHERE expires <= ? AND node IS NULL", (self.now(),))
        db.execute("INSERT INTO invitations(token,network,expires,source) VALUES(?,?,?,?)",
                   (digest(token), network, int(self.now()) + 3600,source))
        return token

    def methods(self, db, network):
        row = db.execute("SELECT link,qr FROM join_methods WHERE network=?", (network,)).fetchone()
        return {"link":bool(row["link"]) if row else True,"qr":bool(row and row["qr"]),
                "password":bool(db.execute("SELECT 1 FROM network_passwords WHERE network=?",(network,)).fetchone())}

    def login(self, name, password):
        if not isinstance(name,str) or not 1 <= len(name.strip()) <= 64:
            raise ApiError(403,"Invalid network credentials")
        login = groups.name_key(name)
        with self.transaction() as db:
            entry = db.execute("SELECT * FROM network_passwords WHERE login=?", (login,)).fetchone()
        verifier = password_hash(password,entry["salt"] if entry else bytes(16))
        if not entry or not secrets.compare_digest(verifier,entry["verifier"]):
            raise ApiError(403,"Invalid network credentials")
        with self.transaction() as db:
            current=db.execute("SELECT network FROM network_passwords WHERE login=? AND verifier=?",(login,verifier)).fetchone()
            if not current:
                raise ApiError(403,"Invalid network credentials")
            return self.mint(db,current["network"],"password")

    def revoke_pending(self, db, network, source):
        rows = db.execute("""SELECT node FROM invitations WHERE network=? AND source=?
            AND redeemed=0 AND node IS NOT NULL""", (network, source)).fetchall()
        for row in rows:
            path = f"/controller/network/{network}/member/{row['node']}"
            if self.directory(db, path)["authorized"]:
                self.directory(db, path, {"authorized": False})
        db.execute("DELETE FROM invitations WHERE network=? AND source=? AND redeemed=0", (network, source))

    def share(self, session):
        with self.transaction() as db:
            network=self.manager(db,session)["network"]
            methods=self.methods(db,network)
            if not (methods["qr"] or methods["link"]):
                raise ApiError(403, "Invitations are disabled")
            return self.mint(db,network,"share")

    def invitation(self, db, token):
        row = db.execute("SELECT * FROM invitations WHERE token=?", (digest(token),)).fetchone()
        if not row or row["expires"] <= self.now():
            raise ApiError(403, "Invalid or expired invitation")
        return row

    def lookup(self, token):
        with self.transaction() as db:
            network = self.invitation(db, token)["network"]
            info = self.directory(db, "/controller/network/" + network)
            return {k:info[k] for k in ("id","name","transport") if k in info}

    def bound_invitation(self, db, token, node, key):
        row = self.invitation(db, token)
        if row["node"] and (row["node"] != node or key != row["session"]):
            raise ApiError(403, "Invitation already used")
        return row

    def redeem(self, token, node, session, nick, prior_session=None, proof=None):
        if not isinstance(node, str) or not re.fullmatch("[0-9a-f]{10}", node):
            raise ApiError(400, "Invalid node")
        key, nick = digest(session), nickname(nick)
        with self.transaction() as db:
            row = self.bound_invitation(db, token, node, key)
            network = row["network"]
            group = groups.directory_group(db, network)
            transport = group["transport"] if group else network
        endpoint = f"/controller/network/{transport}/member"
        if node not in self.transport(endpoint):
            raise ApiError(409, "Waiting for network join")
        verify = not row["device_verified"]
        if verify:
            identity = self.transport(endpoint + "/" + node).get("identity")
            self.verify_device(identity, node, token, session, proof)

        target = f"/controller/network/{network}/member/{node}"
        with self.transaction() as db:
            # За время проверки билет мог истечь, быть отозван или достаться другому узлу.
            row = self.bound_invitation(db, token, node, key)
            member = self.directory(db, target)
            if not row["device_verified"] and (not verify or member.get("identity") != identity):
                raise ApiError(403, "Device identity changed")
            if row["redeemed"] and not member["authorized"]:
                raise ApiError(403, "Membership revoked")
            if not row["node"]:
                if db.execute("SELECT 1 FROM departed WHERE session=?", (key,)).fetchone():
                    raise ApiError(403, "Session was retired")
                reserved = db.execute("""SELECT token FROM invitations WHERE network=? AND
                    node=? AND session=? AND redeemed=0""", (row["network"], node, key)).fetchone()
                if member["authorized"] and not reserved:
                    raise ApiError(403, "Node already enrolled")
                group = groups.group_info(db, row["network"])
                if group and not reserved:
                    claimed = db.execute("SELECT 1 FROM invitations JOIN groups ON groups.id=invitations.network WHERE node=?",
                                         (node,)).fetchone()
                    raw = self.transport("/controller/network/"+group["transport"]+"/member/"+node)
                    if claimed or raw["authorized"]:
                        prior = db.execute("""SELECT 1 FROM invitations JOIN groups ON groups.id=invitations.network
                            WHERE node=? AND session=? AND redeemed=1 AND device_verified=1""", (node,digest(prior_session))).fetchone()
                        if not prior:
                            raise ApiError(403, "Existing device session required")
                if reserved:
                    db.execute("DELETE FROM invitations WHERE token=?", (reserved["token"],))
                try:
                    db.execute("UPDATE invitations SET device_verified=1, node=?, session=?, nick=? WHERE token=?",
                               (node, key, nick, row["token"]))
                except sqlite3.IntegrityError:
                    raise ApiError(403, "Node or session already enrolled")
            elif not row["device_verified"]:
                db.execute("UPDATE invitations SET device_verified=1 WHERE token=?", (row["token"],))
        # Сначала сохраняем привязку. Повтор после сбоя возможен только с той же сессией.
        with self.transaction() as db:
            row = self.bound_invitation(db, token, node, key)
            if not row["redeemed"]:
                self.directory(db, target, {"authorized": True})
                db.execute("UPDATE invitations SET redeemed=1, seen=? WHERE token=?",
                           (int(self.now()), row["token"]))
        return {"id": row["network"]}

    def assign_owner(self, network, node):
        with self.transaction() as db:
            if not db.execute("SELECT 1 FROM invitations WHERE network=? AND node=? AND redeemed=1 AND device_verified=1",
                              (network, node)).fetchone():
                raise ApiError(404, "Participant has no profile")
            if not self.directory(db, "/controller/network/" + network + "/member/" + node)["authorized"]:
                raise ApiError(403, "Membership revoked")
            prior = db.execute("SELECT node FROM roles WHERE network=? AND role='owner'", (network,)).fetchone()
            if prior and prior["node"] != node:
                raise ApiError(409, "Owner already assigned")
            db.execute("INSERT OR REPLACE INTO roles VALUES(?,?,'owner')", (network, node))

    def manager(self, db, session):
        own = db.execute("""SELECT i.network,i.node,r.role FROM invitations i JOIN roles r
            ON i.network=r.network AND i.node=r.node WHERE i.session=? AND i.redeemed=1 AND i.device_verified=1""",
                         (digest(session),)).fetchone()
        if not own or not self.directory(db, "/controller/network/" + own["network"] + "/member/" + own["node"])["authorized"]:
            raise ApiError(403, "Network management permission required")
        return own

    def manage(self, session, action, value):
        with self.transaction() as db:
            actor = self.manager(db, session)
            network = actor["network"]
            endpoint = "/controller/network/" + network
            if action == "name":
                name = network_name(value)
                groups.require_available_name(db,self.transport,network,name)
                self.directory(db, endpoint, {"name": name})
                db.execute("UPDATE network_passwords SET login=? WHERE network=?",(groups.name_key(name),network))
            elif action == "password" or action == "password-mode" and value == "off":
                if action == "password":
                    name=self.directory(db,endpoint)["name"]
                    groups.require_available_name(db,self.transport,network,name)
                    salt=secrets.token_bytes(16)
                    verifier=password_hash(value,salt)
                    db.execute("""INSERT INTO network_passwords(network,login,salt,verifier) VALUES(?,?,?,?)
                        ON CONFLICT(network) DO UPDATE SET login=excluded.login,salt=excluded.salt,verifier=excluded.verifier""",
                        (network,groups.name_key(name),salt,verifier))
                else:
                    db.execute("DELETE FROM network_passwords WHERE network=?",(network,))
                self.revoke_pending(db, network, "password")
            elif action in ("link","qr"):
                if value not in ("on","off"):
                    raise ApiError(400,"Invalid invitation setting")
                db.execute("INSERT OR IGNORE INTO join_methods(network) VALUES(?)",(network,))
                column = "link" if action == "link" else "qr"
                db.execute(f"UPDATE join_methods SET {column}=? WHERE network=?",(int(value=="on"),network))
                methods = self.methods(db,network)
                if not (methods["link"] or methods["qr"]):
                    self.revoke_pending(db, network, "share")
            elif action == "remove":
                if not isinstance(value, str) or not re.fullmatch("[0-9a-f]{10}", value):
                    raise ApiError(400, "Invalid participant")
                role = db.execute("SELECT role FROM roles WHERE network=? AND node=?", (network,value)).fetchone()
                if value == actor["node"] or role and (role["role"] == "owner" or actor["role"] != "owner"):
                    raise ApiError(403, "Cannot remove this participant")
                if value not in self.directory(db, endpoint+"/member"):
                    raise ApiError(404, "Unknown participant")
                if not self.directory(db, endpoint+"/member/"+value)["authorized"]:
                    raise ApiError(404, "Unknown participant")
                self.remove_member(db, network, value)
            elif action in ("role", "transfer"):
                if actor["role"] != "owner" or value == actor["node"]:
                    raise ApiError(403, "Owner permission required")
                if not isinstance(value, str) or not re.fullmatch("[0-9a-f]{10}", value):
                    raise ApiError(400, "Invalid participant")
                if not db.execute("SELECT 1 FROM invitations WHERE network=? AND node=? AND redeemed=1 AND device_verified=1",
                                  (network, value)).fetchone() or not self.directory(db, endpoint+"/member/"+value)["authorized"]:
                    raise ApiError(404, "Participant has no active profile")
                if action == "transfer":
                    db.execute("DELETE FROM roles WHERE network=? AND node=?", (network,actor["node"]))
                    db.execute("INSERT OR REPLACE INTO roles VALUES(?,?,'owner')", (network,value))
                else:
                    removed = db.execute("DELETE FROM roles WHERE network=? AND node=? AND role='admin'", (network,value))
                    if not removed.rowcount:
                        db.execute("INSERT INTO roles VALUES(?,?,'admin')", (network,value))
            else:
                raise ApiError(400, "Unsupported management action")
        return {"id": network}

    def remove_member(self, db, network, node):
        self.directory(db, "/controller/network/"+network+"/member/"+node, {"authorized":False})
        db.execute("INSERT OR IGNORE INTO departed SELECT session FROM invitations WHERE network=? AND node=? AND session IS NOT NULL",
                   (network,node))
        db.execute("DELETE FROM roles WHERE network=? AND node=?", (network,node))
        db.execute("DELETE FROM invitations WHERE network=? AND node=?", (network,node))

    def leave(self, session):
        key = digest(session)
        with self.transaction() as db:
            if db.execute("SELECT 1 FROM departed WHERE session=?", (key,)).fetchone():
                return {}
            row = db.execute("SELECT network,node FROM invitations WHERE session=? AND redeemed=1 AND device_verified=1", (key,)).fetchone()
            if not row:
                raise ApiError(403, "Invalid membership")
            self.remove_member(db,row["network"],row["node"])
        return {}

    def attest(self, session, proof):
        key = digest(session)
        with self.transaction() as db:
            row = db.execute("SELECT network,node FROM invitations WHERE session=? AND redeemed=1", (key,)).fetchone()
            if not row:
                raise ApiError(403, "Invalid session")
            member = self.directory(db, f"/controller/network/{row['network']}/member/{row['node']}")
            if not member["authorized"]:
                raise ApiError(403, "Membership revoked")
            self.verify_device(member.get("identity"), row["node"], row["network"], session, proof)
            db.execute("UPDATE invitations SET device_verified=1 WHERE session=?", (key,))
        return {}

    def sync(self, session, nick):
        key, nick = digest(session), nickname(nick)
        with self.transaction() as db:
            own = db.execute("SELECT * FROM invitations WHERE session=? AND redeemed=1", (key,)).fetchone()
            if not own:
                raise ApiError(403, "Invalid session")
            endpoint = "/controller/network/" + own["network"]
            info = self.directory(db, endpoint)
            member_path = endpoint + "/member/"
            if not self.directory(db, member_path + own["node"])["authorized"]:
                raise ApiError(403, "Membership revoked")
            if not own["device_verified"]:
                return {"id": own["network"], "name": info["name"], "needs_proof": True, "members": [],
                        "methods": {"link":False, "qr":False, "password":False}}
            db.execute("UPDATE invitations SET nick=?, seen=? WHERE session=?", (nick, int(self.now()), key))
            profiles = {r["node"]: r for r in db.execute(
                "SELECT node,nick,seen FROM invitations WHERE network=? AND redeemed=1", (own["network"],))}
            roles = dict(db.execute("SELECT node,role FROM roles WHERE network=?", (own["network"],)))
            rows = []
            for node in (profiles if "transport" in info else self.directory(db, endpoint + "/member")):
                member = self.directory(db, member_path + node)
                if member["authorized"]:
                    profile = profiles.get(node)
                    rows.append({"node": node, "role": roles.get(node, "member"), "ip": next(iter(member["ipAssignments"]), ""),
                                 "nick": profile["nick"] if profile else node,
                                 "present": bool(profile and profile["seen"] > self.now() - 45)})
            return {"id": own["network"], "name": info["name"], "members": rows, "methods": self.methods(db,own["network"])}
