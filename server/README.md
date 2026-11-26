# Self-hosted Uplink instance

The Windows app connects to the VPS over SSH to set up a network. Members use an HTTPS invitation or the server address, network name and password. Each successful enrollment gives the member a private IPv4 address and a separate EasyTier credential. The server keeps the network secret. Leaving or removing a member revokes that credential. Membership has no expiry. Device credentials last 30 days. At the first sync after expiry, the server checks membership again and returns a fresh credential for the same node and IP in the HTTPS response. A saved session is sufficient even when the original invitation has expired.

## Deployment

Use Debian 12 or Ubuntu 24.04 on x86-64 with systemd, Python 3 and root or passwordless sudo access. Allow TCP 80/443 for certificate issuance and the client API, plus UDP 9993 for the tunnel. The installer verifies the pinned EasyTier 2.7.1 Linux archive and both executable hashes. A Windows installation includes its own engine and Wintun.

An existing provider HTTPS proxy can forward to the private client API on port 8930. Forward the advertised external UDP port to UDP 9993. On the confirmation screen, enable the hosting provider’s HTTPS proxy and enter its hostname, the private VPS address and the forwarded public UDP port. The SSH deployment API also accepts the provider's public hostname, external UDP port and private HTTP bind address for installations behind NAT.

## Update an existing VPS

In Uplink 0.3.0, open **⋮ → Update server** from a network you own. Enter the SSH login and a password or key with root or passwordless sudo access. The app checks the address registered on the VPS before stopping anything. It updates the existing instance, including all networks on it. The connection pauses while the services restart. When the new server fails its health check, Uplink restores the previous program files and data, then checks the restored services.

The VPS continues the update if your SSH connection drops. Open the same action again to reconnect or recover an interrupted update. **Copy update log** shares the timeline from the current attempt. Windows saves it in `%APPDATA%\Uplink\server-upgrade.log`. The VPS keeps a copy in `/var/lib/uplink-upgrade/<attempt>/events.jsonl`. If automatic recovery fails, the recovery snapshot is kept on the VPS.

For scripts or agent use, run `uplinkctl vps upgrade` with a JSON request on standard input. The `server` field is the address shown in the network menu. The `access.ip` field is the address you SSH into, which can differ when your host uses a proxy. Supply either `password` or `identity`, a path to the SSH private key. Provide `fingerprint` for the first connection or set `trust_on_first_use` to `true` to use the app’s normal first-connection policy.

```json
{
  "server": "lan.example.com",
  "access": {
    "ip": "192.0.2.10",
    "port": 22,
    "user": "root",
    "identity": "C:/Users/you/.ssh/vps_ed25519",
    "trust_on_first_use": true
  }
}
```

The CLI returns JSON with the outcome and the log path. An updated or current instance returns exit code 0. A failed or rolled-back update returns a nonzero code. The update targets the server revision bundled with the app. Uplink 0.3.0 bundles server v3 and updates instances on EasyTier 2.7.1. The network menu shows the reported server version. Owners get an update notice when a newer bundled revision is available. The app keeps using features supported by the connected server and rejects downgrades.

## Maintain server versions

Increment `revision.txt` when a server release changes the installed bundle. The Windows app can ship the same server revision across consecutive releases. Keep existing API responses compatible and advertise new capabilities separately so the app can use them when present. Verify both a fresh installation and an in-place update from the preceding server release.

## Runtime

| Component | Responsibility |
| --- | --- |
| `/opt/uplink/bin` | Pinned EasyTier core and administrative CLI |
| `uplink-engine.service` | Common UDP listener and separate credential authority per network |
| `uplink-api.service` | Handles invitations, per-member sessions and network management |
| `/run/uplink-owner.sock` | Owner API for authenticated local provisioning |
| `/var/lib/uplink-members` | SQLite membership, invitation digests, roles and network settings |
| `/var/lib/uplink-engine` | Private engine configuration and credential stores |
| `uplink-rpc-guard.service` | Restricts the local engine management port to root and the service account |

Uplink reserves a separate private /24 per network on the VPS. The relay allows the configured network IDs. When Linux provides a TUN device, the VPS also participates in its virtual subnets. The HTTPS API works independently of host participation in a subnet. Back up both state directories together while the services are stopped. Keep the backups private.
