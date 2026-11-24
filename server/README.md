# Self-hosted Uplink instance

The Windows app connects to the VPS over SSH to set up a network. Members use an HTTPS invitation or the server address, network name and password. Each successful enrollment gives the member a private IPv4 address and a separate EasyTier credential. The server keeps the network secret. Leaving or removing a member revokes that credential. Membership has no expiry. Device credentials last 30 days. At the first sync after expiry, the server checks membership again and returns a fresh credential for the same node and IP in the HTTPS response. A saved session is sufficient even when the original invitation has expired.

## Deployment

Use Debian 12 or Ubuntu 24.04 on x86-64 with systemd, Python 3 and root or passwordless sudo access. Allow TCP 80/443 for certificate issuance and the client API, plus UDP 9993 for the tunnel. The installer verifies the pinned EasyTier 2.7.1 Linux archive and both executable hashes. A Windows installation includes its own engine and Wintun.

An existing provider HTTPS proxy can forward to the private client API on port 8930. Forward the advertised external UDP port to UDP 9993. On the confirmation screen, enable the hosting provider’s HTTPS proxy and enter its hostname, the private VPS address and the forwarded public UDP port. The SSH deployment API also accepts the provider's public hostname, external UDP port and private HTTP bind address for installations behind NAT.

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
