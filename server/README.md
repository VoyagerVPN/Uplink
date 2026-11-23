# Uplink test instance

The Linux instance runs a private ZeroTier root and controller, a local API for the instance owner and a client HTTP service behind a TLS proxy. ZeroTier uses UDP for transport. The client API supplies connection details, verifies invitations and synchronizes member lists. Only root can access the owner's Unix socket.

## Installation paths

| Server path | Purpose |
| --- | --- |
| `/opt/uplink-zt/1.16.2/usr/sbin/` | Official ZeroTier Linux binaries from the vendor's signed repository |
| `/opt/uplink-zt/instance.py` | Uplink API handler from this directory |
| `/etc/systemd/system/uplink-*` | Service and socket units from this directory |
| `/var/lib/uplink-zt` | Controller state, node identity, local token and signed `planet` file. Owned by the `uplink-zt` system user with mode `0700` |
| `/var/lib/uplink-zt-signing` | Private material for signing root configuration updates. Only the test instance owner should have access |
| `/run/uplink-instance.sock` | Local owner API |

A test client needs the signed public `planet` file and the network ID. Replacing `planet` in an existing ZeroTier installation affects every network it uses. Test clients use separate state directories. Each client generates its own key pair, presents a single-use invitation and waits for the controller to register its join request. The owner can also manage membership through the Unix socket. Admission requires authorization even if the client already has the public file and network ID.

## Create a network from Windows

In Uplink 0.2.0 choose **Join network → Create a network on my VPS**. Enter the network name, IPv4 address, SSH port and login. Authenticate with a VPS password or an SSH identity file. The client verifies the Ed25519 host key against the SHA256 fingerprint from the provider before sending credentials. Root or passwordless sudo is required. Saved host fingerprints and pending creation IDs are local to the Windows profile. Uplink keeps SSH credentials in process memory.

For a new public IPv4 server, use a clean Ubuntu 24.04 or Debian 12 x86-64 installation with systemd, Python 3 and free TCP 80/443 and UDP 9993. Allow these ports in the provider and host firewalls. The installer verifies the pinned vendor package, sets up a private ZeroTier controller and starts the Uplink services. Nginx serves the HTTP-01 challenge and forwards HTTPS to the loopback client API. Certbot requests a short-lived IP certificate and a systemd timer renews it twice daily. The remote package source must be reachable. The private controller does not need a TAP/TUN device.

The owner joins using the regular device proof flow. The installer assigns ownership through the root-only socket and returns a separate invitation for friends. After an interruption, retry the same name on the same VPS to reuse the pending network. Later networks reuse the existing controller. Windows installations already using another private ZeroTier root stop before replacing that root.

For headless use, send a JSON request to **uplinkctl vps create** on stdin. Required fields are `name`, `ip`, `port`, `user`, `fingerprint` and either `password` or `identity`. The caller receives the friend invitation on stdout after enrollment. Keep credentials and returned invitations out of logs. Headless owner enrollment requires a previously prepared Windows transport.

If the host already provides trusted HTTPS and port forwarding, the CLI accepts an existing `public_host`, private `proxy_bind` and external `udp_port` for first installation. Forward HTTPS to `proxy_bind:8930` and the external UDP port to 9993. Subsequent GUI runs on this endpoint read its saved configuration. Provider panel automation is separate.

## Check a VPS from Windows

For the first deployment target, use a Linux VPS with a public IPv4 address and SSH key authentication. The development CLI runs a read-only preflight over one SSH connection:

```powershell
uplinkctl vps check 192.0.2.10 22 ubuntu
```

The preflight uses Windows OpenSSH, its default identity files or agent, and its known hosts. To select specific files, append `--identity FILE` or `--known-hosts FILE`. Verify the SSH host fingerprint with the VPS owner before adding it to the trust store. The report includes the OS, architecture, systemd as PID 1, the TUN device, Python 3, the privilege probe and the presence of a Uplink runtime directory. An SSH success can return `base_checks_passed: false`; this flag covers local prerequisites only. External UDP, HTTPS and installation of the services require separate verification.

## Prepare a private root on a new Linux host

With the verified ZeroTier One 1.16.2 binary installed at the path above, run this as the instance owner with root permissions. Substitute the public IPv4 address and externally mapped UDP port for the documentation example.

```sh
python3 prepare_root.py /var/lib/uplink-zt-signing 192.0.2.10 30000 --runtime /var/lib/uplink-zt
```

Create the `uplink-zt` system user first. With `--runtime`, the command needs a trusted parent directory and initializes a new controller directory with `identity.secret`, `identity.public` and `planet`, owned by the service account with mode `0600`. Subsequent runs verify those files against the signing bundle and preserve existing networks. A missing signing bundle for an existing runtime, mismatched files or a changed endpoint stops preparation. Keep the signing directory, including `world.json`, in a root-only backup. Omit `--runtime` to prepare the signing bundle alone. For the complete installation use the Windows creation flow above.

## Owner API

Read the instance state on the server with the following command.

```sh
curl --fail --unix-socket /run/uplink-instance.sock http://localhost/v1/instance
```

| Method | Resource | Purpose |
| --- | --- | --- |
| GET | `/v1/instance` | Return the controller ID and its networks |
| PUT | `/v1/networks/{network_id}` | Create a private network with `{"name":"Test","subnet":"10.23.42.0/24"}`. The controller supplies the first 10 hex digits of the ID and the owner chooses the last 6. Retrying with the same ID and configuration returns 200. A conflict returns 409 |
| GET | `/v1/networks/{network_id}` | Read the name, privacy setting, routes and broadcast setting |
| GET | `/v1/networks/{network_id}/members` | List nodes that have requested membership |
| GET, PUT | `/v1/networks/{network_id}/members/{node_id}` | Read membership or set `{"authorized":true}` or `{"authorized":false}` |
| POST | `/v1/networks/{network_id}/invites` | Issue a single-use invitation valid for one hour |
| PUT | `/v1/networks/{network_id}/owner` | Assign the first network owner. The body contains the registered member's `node`. The current network owner handles subsequent transfers |

## Multiple groups on one adapter

Create the first logical network through the local owner socket. Generate a unique 32-digit hexadecimal group ID and keep it for retries. The request body only needs the network name.

```sh
curl --fail --unix-socket /run/uplink-instance.sock -X PUT -H 'Content-Type: application/json' \
  -d '{"name":"After Hours"}' \
  http://localhost/v1/groups/cc019d6a319d4f9a8bc188529115fda1
```

For another group, use a fresh ID and a different name. The first request allocates a shared ZeroTier transport and a private /24 in `10.0.0.0/8`, skipping IPv4 routes already registered with this controller. Subsequent groups reuse the allocation. To avoid a conflict with a physical LAN known to the owner, set both `transport` and `subnet` explicitly on the first request. Legacy standalone networks keep their connections and invitations. Controller 1.16.2 supports up to 128 signed tags per network. Uplink uses one tag per group.

Use `/v1/networks/{group_id}/invites` to issue invitations and `/v1/networks/{group_id}/owner` to assign the first owner. Roles, names and member lists belong to individual groups. During bootstrap, the Windows client sends `version: 2`, stores a separate group session and joins the returned 16-digit transport ID. When joining a second group, the client proves node ownership with the session of a group it already belongs to on that instance. `POST /v1/leave` takes the session of the group being left. Leaving the last group revokes the node's authorization on the shared transport. Controller rules allow frames through only when both endpoints share an active tag.

## Client API and deployment

Install `instance.py`, `groups.py`, `invites.py`, `device_proof.py` and `client_api.py` in `/opt/uplink-zt/` with root ownership. Install the systemd units from this directory. For `uplink-client-api.service`, set `UPLINK_BIND` in `/etc/uplink-client-api.env` to the container's internal IP reachable by the trusted reverse proxy. Forward HTTPS requests to TCP port 8930 in that container. On instance 129, a separate ServerHQ proxy terminates TLS. Clients verify the certificate. Keep the controller API and owner socket local. To restore an instance, keep separately protected backups of the controller state, signed root configuration and `/var/lib/uplink-instance/` (sessions, profiles and roles).

1. The instance owner creates a network through the Unix socket, then issues an invitation with POST. The link format is `uplink://<HTTPS-host>/invite/<token>`. The test instance requires no additional paid proxy.
2. The Windows client sends the token in the JSON body of `POST /v1/bootstrap`, checks the public planet against the local configuration and asks the local ZeroTier service to join.
3. `POST /v1/redeem` binds the invitation to a pending node ID and a separate random client session. Retrying with the same session is safe. The database stores hashes. The server rejects reuse of the link by another client and attempts to claim an already authorized node's profile.
4. `POST /v1/sync` checks membership, updates the nickname and returns the network and its members. ZeroTier nodes without Uplink appear by ID. The background process continues to deliver updates when the window is minimized to the tray. Session presence expires after 45 seconds. An ICMP response confirms node reachability.
5. `POST /v1/manage` checks permissions for renaming, removing members, changing roles and transferring ownership. The network owner can appoint admins and transfer ownership. Owners and admins can use `POST /v1/share`. Creating networks on the instance requires access to the owner socket.
6. Your row shows the response time for `POST /v1/ping` after the TLS handshake. Windows measures peer latency with ICMP from the corresponding virtual interface's address. An unavailable measurement has no numeric value.

Run server tests from the project root with `python -B -m unittest discover -s server`. Windows requires ZeroTier 1.16.2 configured with the instance's root. See the [checklist](../UX-CHECKLIST.md) for known limitations and outstanding tests. The Uplink sources exclude controller binaries and private keys. [ZeroTier controller distribution terms](https://github.com/zerotier/ZeroTierOne/blob/1.16.2/nonfree/LICENSE.md)

## Network avatars

Owners and admins upload the cropped 512 × 512 avatar through `POST /v1/avatar`. Allow request bodies up to 2 MiB on this route in the HTTPS proxy. The image contains zlib-compressed RGBA pixels, encoded as base64. The server checks membership, role and decoded size before storing it.

Clients send their cached `avatar_revision` in the regular sync request. An empty string requests the current image. The reply includes the image only when its SHA-256 differs. Clients that omit the field keep their original sync response. If the server omits the revision, the client shows the server update hint in network settings.

## Join with a network name and password

Enter the server's HTTPS host, network name and password. On the same instance, “Coop” and “ coop ” identify the same network. Separate instances can use the same name. Creating or renaming a network with a name already in use returns HTTP 409. Renaming preserves the ID, memberships and existing invitations. If a legacy installation already has duplicate names, the owner must choose a new name before enabling password sign-in.

An owner or admin sets the password in invitation settings. The client sends the name and password in the HTTPS body of `POST /v1/login`. The server checks a separately salted scrypt verifier and issues a single-use ticket valid for an hour. Changing or disabling the password revokes pending sign-ins made with the old password. Existing memberships remain active. Each instance allows up to 12 login attempts per minute and two concurrent password checks. Exceeding either limit returns 429. The QR code and invitation field contain the same ticket.

On first join, the client must sign the ticket and session with a device-specific key. The server obtains the trusted identity from its own controller. When upgrading a Windows installation, rerun `install-transport.ps1` once for the current user with an invitation for this instance. The script uses the adjacent `transport/windows/Provision-DeviceProof.ps1`. The node key stays with the service. The client uses a separate key with restricted access. Existing memberships use `POST /v1/attest` for verification. Until verified, the server hides the member list and blocks management actions.

## Troubleshooting a join

On the VPS, run `journalctl -u uplink-client-api.service --since "10 minutes ago" -o cat`. A password join starts with `login`. Both join methods then use `bootstrap` and `redeem`. A successful `bootstrap` followed by silence can indicate a local setup failure. Check the Windows client's `%APPDATA%\Uplink\last-join.log` for the last attempted stage.

The server records the operation, HTTP status and a bounded reason code. A short keyed ticket tag links requests for the same invitation until the API restarts. Devices opening the same invitation share that tag. Requests without an invitation use `ticket=-`.

The Windows log covers the most recently started GUI or CLI join. Each line begins with a UTC Unix timestamp in milliseconds and elapsed milliseconds. The header records the client version and join method. Find the last `result=failed` line for the operation and error kind. Look immediately above it for an HTTP status, curl code or process exit code. A Windows error can include `win32` on the failure line. For example, `operation=read_planet kind=NotFound win32=2` means the client could not find its local ZeroTier root file. Only predefined reason text is recorded. The log omits credentials, URLs, file paths, identities and request or response bodies.
