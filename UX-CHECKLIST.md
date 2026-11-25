# Uplink UX checklist

Checked items record verified behavior. The early UX checks used the Windows client with a local network model. Later sections record tests with the live transport.

## Current transport verification

- [x] The Windows package bundles the Uplink service, EasyTier 2.7.1 and Wintun. A restricted Windows client joined by invitation, received a virtual IPv4 address, exchanged UDP with a Linux member, and left. One HTTPS connection and one UDP flow carried 23 TCP and 25 UDP packets on the local test network.
- [x] Instance 129 runs the new engine, owner socket, HTTPS API and local RPC guard. Windows and Linux joined from separate invitations through the public endpoint, exchanged a UDP nonce, and left. Captured during admission and teardown: 134 TCP and 125 UDP packets in both directions. Peak across deployment and testing: 43 combined packets in a rolling second.
- [x] Verify two independent network authorities on one Windows adapter locally. Adding and leaving network B kept A reachable in 95/95 UDP probes, with the same adapter GUID and engine PID throughout.
- [x] A restricted Windows client redeemed invitations from two independent local HTTPS instances. The packaged service added a second virtual IPv4 to the same adapter. Minecraft Java 26.3 showed both test LAN announcements together on its Multiplayer screen. Leaving the second network preserved the first address. The A-side UDP probe received 97/97 replies with Multiplayer open before joining, and 94/94 when launching the game with both networks already connected.
- [ ] Repeat simultaneous membership across two independently hosted public VPS instances.
- [ ] Verify discovery and direct connectivity from two physical Windows PCs behind separate routers.
- [x] Verify lifetime membership. A restricted Windows client reconnected to a local Linux peer after its 17-second test credential expired and exchanged UDP on its original IP. Server tests cover 183 days offline, a lost renewal reply, role retention and refusal after removal or leave.

Earlier dated measurements and release notes below refer to the transport shipped with those releases.

## Release plan

- **0.1.0** is the published Windows installer for joining existing instances.
- **0.1.1** covers the current client, transport and server fixes, including HTTPS connection reuse during background sync.
- **0.1.2** gives release notes a compact scrollbar with a dedicated gutter.
- **0.1.3** shares network avatars and translates connection and settings errors.
- **0.2.0** covers VPS setup from the Windows UI, from entering SSH credentials through creating the first network and receiving its invitation.
- **0.3.0** adds member-to-member SSH access with explicit consent on the receiving PC. Resume two-PC Minecraft LAN discovery diagnostics after the SSH workflow is available.

## Port of the approved HTML prototype to Rust

- [x] At 388 × 540, the network rail starts at (20, 116). The first icon is 32 × 32 and icons are spaced 41 px apart. The plus and the invitation field's close icon are centered in the same column.
- [x] The selected network name and server address start at x = 75. Table headers start at y = 183 and the 32 px rows start at y = 209. The title-bar sliders open app settings.
- [x] Invite links open all five HTML demo networks. Select a saved network in the rail to switch views.
- [x] The network gear opens Overview, Invitations and Members. Each test network stores its own changes.
- [x] Choose an avatar with the Windows file picker or drop an image. The editor shows a rounded preview and supports panning, zooming and replacing the source. It saves a 512 × 512 square.
- [x] Share a network avatar with verified members. Owners and admins can replace it. A regular sync transfers the image only when the local copy is missing or outdated. Older instances show a server update hint before photo selection.
- [x] Verify an avatar upload and replacement in two running native Windows client windows, with independent owner and member profiles and the local HTTPS membership server. Both rendered the same 512 × 512 image on their next UI refresh. An open nickname editor also keeps receiving rail updates.
- [ ] Repeat the avatar check against the deployed VPS from two installed PCs.
- [x] The invite dialog shows enabled methods. QR codes and links can be enabled independently. Password sign-in also asks for the server address and network name.
- [x] Owners and admins manage the name, avatar and invitations. Owners change roles and transfer ownership. Admins can remove regular members. Removal and transfer require confirmation.

## Invitations and identity

- [x] Opening an invite joins the specified network without an account, username or password.
- [x] Copy an invite link from the network. The invite field and join icon share one row without a separate label. Press Enter or click the icon to open the link.
- [x] The first launch shows the Windows username. An edited nickname survives a restart.
- [x] Hover over the nickname to reveal a pencil on the right. Clicking it opens a bordered inline field with the same font and puts the caret at the end without selecting the text. Enter or a click outside saves the nickname and exits editing. Escape discards changes. Clicking another control also performs its action on that first click. The header and table reflect the saved nickname immediately.
- [x] Both fields support Ctrl+V with a Russian keyboard layout. The focused invitation field uses a neutral border without a blue underline.
- [x] Leave removes membership. Closing the window keeps the network and leaves Uplink in the tray.

## Main window

- [x] Separate window fixed at 388 × 540 logical pixels.
- [x] The title is Uplink. The UI uses “members” and omits old demo labels.
- [x] The compact profile header shows the nickname and a global power switch on the right. The virtual IP appears in the selected network's table.
- [x] Below the profile, network icons form a column on the left. The selected network's name, server address, invite and leave buttons sit to its right.
- [x] Only the table shows the crown beside the creator's nickname. Owners and admins rename the network with the Overview pencil. Enter or a click outside saves. Escape cancels. Rejoining keeps the name.
- [x] Member and network lists scroll independently. Ping, Members and IP appear from left to right, with headings and values aligned to the start of each column.
- [x] Click Members or Ping to sort. Click again to reverse the order. Members without a ping measurement appear last in both directions when sorting by latency.
- [x] Hover over your own or another member's IP to reveal the copy icon. Copying keeps the address visible and shows a check mark for two seconds, even if the pointer leaves. Clicking again restarts the timer.
- [x] Ping is the only connection indicator in each row. Three gray bars indicate a missing measurement. Tooltips distinguish your server latency from latency to each peer.

- [x] Main-screen buttons are transparent at rest. Standard buttons share a gray hover background. Sort headers brighten their text and arrow. Edit pencils show only the icon. Hovering over the offline state brightens the cable and label. Mouse highlights clear when the pointer leaves. Tab focus stays visible.

## Power and settings

- [x] The global button beside the nickname toggles the client and preserves memberships. Gray means off, green means healthy, orange means a problem and red means a critical error. Hover for the cause. The local test model can simulate connection problems.
- [x] The settings control stays highlighted while Settings is open. The nickname and power panel is hidden there. A back arrow to the left of Settings has a Back tooltip. The arrow or settings control returns to the main screen. Start at sign-in uses the current Windows user. Auto-connect applies when the tray process starts and respects a previous manual shutdown.
- [x] The automatic update preference survives a restart.
- [x] The native title bar shows the build version and update indicator to the right of Uplink. Download, progress and verification circles have a 7.5 px radius and a 1.5 px stroke. The accent is `#BFEC70`. Progress animates to the received percentage. An arc rotates during verification. Click the version for the available release notes. Installation requires a separate click.
- [x] Stable updates use GitHub Releases, a signed manifest and the standard installer. Each release starts as a draft with English notes and requires owner approval to publish.
- [x] The indicator reflects actual adapter and connection diagnostics. On instance 129, stopping the API produced warning and stopping the Windows ZeroTier service produced critical. Both returned to ok when the services restarted.

## Icons and preferences

- [x] Settings icons share a column. Windows marks start at sign-in, chain links mark auto-connect, a cloud marks updates and a globe marks language. All labels align. The borderless language button uses its background to show keyboard focus.

- [x] The custom language selector offers Russian and English. Its borderless menu left-aligns names and marks the active language in green with a check on the right. Switching translates the open window immediately and survives restart. Network names and nicknames retain their spelling. With the menu open, one click still toggles a switch, goes back or opens app settings.
- [x] Labels, tooltips, context menus and client messages use the language catalog. Additional catalogs appear in the selector after a build.

- [x] The three green bars are embedded in the executable and appear as the window icon and title-bar mark.
- [x] Windows Explorer shows a live Uplink thumbnail on taskbar hover with the three green bars in its header and on the taskbar button.
- [x] The tray remembers its mode: brand green, ping indicator or Windows theme. The tooltip reads “Uplink”.
- [x] The Uplink tray menu previews the three icon modes and marks the current one. Icon and power changes update it in place while Explorer's hidden-icons panel remains open. The menu remains visible when the main Uplink window receives focus. Escape, clicking outside Uplink or clicking the tray icon again dismisses it. Opening the main window and quitting are separate actions.
- [x] A neutral icon represents an unavailable measurement. All three modes were applied to the Explorer tray icon.
- [x] Draw the packaged green, latency, offline and system tray icons on light and dark backgrounds using Windows DrawIconEx at 16, 20, 24 and 32 px. Light-background active strokes measured at least 4.46:1. The menu previews follow the same palettes.
- [ ] Manually check the light Explorer taskbar with real latency samples. On a dark taskbar, the green and Windows-theme icons were legible in the Explorer overflow. Demo ping mode displayed green bars for the 38 ms sample.
- [x] Components and layouts can be edited directly. The library choice was evaluated with a real window displaying Cyrillic and a memory measurement.
- [x] Compare the window and total working sets with the Win32 client while viewing a network and after closing the window.

## Terminology

- [x] The UI calls the membership space a “network”. “Server” refers to the endpoint for the server ping.

## Multiple networks

- [x] Saved memberships are independent of power. When off, the area below the profile centers a muted outline of an unplugged cable and the label “Uplink is off”. Only the cable and label brighten on hover. Clicking restores the full list and the last selected network.
- [x] My Networks is accessible from the main screen. Each entry shows the name and server. One click opens the members. Switching preserves the order. Join opens and focuses the invite field. Two local networks use distinct IPs, member lists and links. Reopening an invitation selects the existing network. Leave removes only the selected membership.
- [x] The settings gear uses a square icon area. Switches use the Uplink accent and the same 6 px radius as buttons. Hovering over the track or thumb highlights it and shows a hand cursor.
- [ ] Check saved networks on different Uplink instances, with independent membership authorities.

Multiple Uplink networks can be active simultaneously. Selecting a network in the rail opens its members while keeping connections active. Each network uses its own EasyTier membership authority. Peers without a shared network must receive neither direct packets nor LAN announcements from one another.

- [x] Three Linux clients running ZeroTier 1.16.2 shared one transport, with one peer in groups A and B. Direct UDP and LAN announcements passed in both directions on one interface only between peers in a shared group. Removing and restoring group B membership preserved the common peer's interface. Clients used the private planet from instance 129 and a separate local controller.
- [x] The API and Windows client distinguish logical networks from the shared transport. Two logical networks were deployed on instance 129. Server checks cover invite joins, member lists, management and revocation of an individual membership. The installed Windows client supports both network formats.
- [x] On the test Windows installation, the standard CLI disabled the shared adapter in 1.02 s and enabled it in 0.83–0.89 s with the same GUID. Two previously created interfaces stayed enabled. These timings measure the device state change.
- [ ] Verify the installed window and CLI against instance 129: check both lists, a stable GUID and uninterrupted UDP in A when leaving and rejoining B. At 01:23 Moscow time on November 22, public HTTPS and SSH on 129 did not respond and Windows did not receive the shared transport configuration.

## Adapter lifecycle

- [x] On application startup, queue the network service start before refreshing a saved membership when Uplink and auto-connect are on. Preserve a pending power-off request and skip deleted or denied memberships. Verified the stopped-to-running transition with an isolated Windows service.
- [x] Use `Uplink` as the Windows adapter name and `Uplink Tunnel` as its description in the Wintun prototype. Windows supplies the Network Connections icon.
- [x] Verify delegated start/stop on a temporary Windows service with a restricted, medium-integrity client. Before delegation Windows denied access. After delegation three start/stop cycles opened a real Wintun session and removed the temporary adapter on stop. The host had UAC disabled.
- [x] Verify the installed Windows CLI against a secure Linux peer using a per-member EasyTier credential. A restricted process completed three on/off cycles, received a UDP reply after each start, and saw the adapter removed after each stop. The administrator staged the profile before the test.
- [x] Join by invitation with a restricted Windows CLI process through the installed service and verify peer UDP traffic. The desktop and CLI use the same enrollment implementation.
- [ ] Register the new transport service as `UplinkNetwork` with display name and executable description `Uplink Network Service`. Keep this service ID across updates. Check installation, reboot and repeated toggles with UAC enabled.

- [ ] Implement one persistent virtual Uplink adapter per Windows installation. On clean Windows, verify that power cycles, network changes and restarts preserve the name and GUID without creating additional devices.
Earlier results with one adapter per network:

- [x] Repeat power cycles and client restarts with one and two networks. Each active network had its own adapter. Two power cycles and a restart in each configuration preserved interface names without accumulating devices. Windows showed zero ZeroTier adapters when off and one or two when on. Recreating a device changed its GUID.
- [x] Measure from clicking Enable Uplink to the first successful packet exchange with a peer's virtual IP. The target was 30 s including adapter creation and address assignment. From the installed Windows button to the first Linux UDP response, one network took 5.04 and 7.25 s and two networks took 8.32–10.88 s.

## Server and transport

- [x] Prototype a two-party SSH access request in the UI. The Administrator switch defaults to on for each request. Turning it off selects standard user access. The recipient chooses a validity period (until revoked, 15 minutes, one hour or one day). Browser checks cover rejection, cancellation, grant, persistence, expiry, closing from either side and requesting again.
- [x] In the SSH prototype, place the active terminal badge after the owner crown or administrator shield beside the nickname. Keep the IP address in its own column.
- [x] Hovering anywhere on a participant row reveals its vertical ellipsis and IP copy icon. Keyboard focus also reveals the actions. Keep the ellipsis visible while its menu is open. Label menu items with a terminal, pencil or remove-member icon. Preserve the copy check mark for two seconds and keep the active SSH badge visible at rest.
- [x] Give every participant a vertical ellipsis in the main table. Members can request SSH access to another participant. Owners and administrators can also rename or remove participants within their existing role permissions. A network nickname is shared within that network and leaves the Windows profile and other networks unchanged. The self menu allows changing one's network nickname.
- [x] Exercise Windows OpenSSH on loopback with disposable keys and a fixed test command. Verify rejection before consent, a successful certificate login, rejection after revocation or expiry, a fresh key on reapproval and termination of an established session in a Windows job.
- [ ] Connect authenticated member requests to native Windows consent and per-grant SSH credentials. Each selected Windows privilege level must be verified with a real session. Display active access only after local activation succeeds. Enforce revocation and expiry for existing and future sessions, including across restart and network leave. Verify with two consenting PCs.

- [x] Generate a private Linux root from the supplied UDP endpoint with the stock ZeroTier 1.16.2 binary. Confirm repeated setup preserves the keys and signed planet and refuses an incomplete bundle or endpoint change.

- [x] On a fresh isolated Linux instance with ZeroTier 1.16.2, create two logical networks on the owner socket by name alone. Verify a single shared transport, idempotent retries, duplicate-name rejection, bootstrap of an invitation against the generated private planet, and rejection of network creation on the public client API.
- [x] Run the Windows installer silently in an SSH service session on a second physical PC. Verify a stopped window and exercise the development CLI's headless join with an isolated demo profile. This checks the unattended UI path on one LAN. The live two-client test below remains open.

- [ ] Let the owner create their first network on a rented VPS from the Windows client. After the owner grants access, a single action installs the Uplink server, creates the network and returns an invitation for friends. Subsequent networks reuse the installed server.
- [x] Carry the prototype's field validation into the Rust/Slint setup screens. On submission, show red text directly below each invalid field and focus the first one. Clear the message when corrected. Use the same behavior for Enter and the submit button.
- [x] Exercise the owner and friend journeys in an offline GUI prototype, including first-use host verification, a second network on the same VPS and joining by invitation. SSH access and installation are simulated in this check.
- [x] Start with a generic public IPv4 VPS over SSH. The Windows CLI checks basic Linux prerequisites using a trusted SSH host key. Checked a successful authenticated probe and rejection of both unknown and changed keys on a loopback SSH fixture.
- [x] The Windows creation wizard goes from VPS access directly to confirmation. It remembers the first SSH host key per IP and port and blocks a changed key before authentication. Behind a hosting provider's HTTPS proxy, the owner can supply the proxy hostname, private address and forwarded UDP port on the confirmation screen. Verified the native form and both SSH paths on loopback.
- [x] Initialize or verify the controller runtime from the signed root. On an isolated Linux controller a repeat preserved the identity, private planet, network and an existing invitation across a restart.
- [x] Connect the native wizard and headless CLI to SSH deployment, first-owner enrollment and a separate friend invitation. The SSH key is pinned before authentication. Retry keeps the pending group ID. The installer supports a generic public IPv4 or an existing reverse proxy.
- [x] Animate the active VPS setup step. Show the created network name and editable avatar next to its invitation. Closing or saving the crop returns to the same invitation.
- [x] Reinstall on service 129 behind the existing proxy. Check invitation bootstrap over public HTTPS and a nonce from external UDP 30000 to internal UDP 9993. Separate Linux peers on an isolated bridge confirmed owner/member roles and UDP echo after interrupting and resuming network creation.
- [ ] Run the full installed Windows owner and friend journey on a dedicated public IPv4, including issuance and renewal from a publicly trusted CA. Provider panel integration is deferred.

Uplink's test instance is ServerHQ service 129.

Only the instance owner can create networks on that instance. They become the first network owner and can transfer ownership to a member. The transfer applies only to that network. A regular invitation grants the member role.

- [x] Check permissions through the server API. On 129 the instance owner created a private network through the Unix socket. A member, an admin and the new network owner after a transfer each received HTTP 403 on a PUT to create another network. The controller confirmed that the requested network had not been created.

Uplink is for private networks among friends. The project aims to publish the Windows client and server source. The owner hosts the server and members manage networks from the Uplink GUI.

- [x] On service 129, verify UDP forwarding to the private root, start the private controller and create a private network through the Uplink API. Address assignment was denied before approval, succeeded after approval and the network survived a service restart.
- [x] Check initial peer discovery without public ZeroTier roots. Two fresh Linux clients in separate Docker networks started with the signed `planet`. Instance 129 supplied the only external IPv4 route. Neither client had an external IPv6 route.
- [x] Test an established connection between two Linux clients. TCP, UDP and UDP broadcast passed both ways over the virtual interfaces. All 30 messages arrived in the final run via instance 129. Results immediately after joining and after restarting were also recorded.
- [ ] Measure broadcast readiness immediately after joining. On the first run in a new network, all five packets were lost in one direction. On retry, all five arrived in each direction. Over three Linux rejoins, the first broadcasts in both directions arrived 0.45–3.31 s after the timestamp taken before join. The upper bound and reason for the initial loss remain open.
- [ ] Repeat joining, bidirectional TCP and UDP, UDP broadcast and latency measurement with two clean Windows Uplink clients on different physical networks. Record whether the route is direct or relayed.
- [x] Confirm that a friend can enter a real player-hosted Minecraft world over Uplink. On 2026-11-24 the host reported a successful join. The world did not appear through automatic LAN discovery in that session.
- [ ] Verify automatic LAN world discovery in an unmodified Minecraft Java client on Windows. Open a world to LAN in each of two independent Uplink networks and a third on the physical LAN. All three must appear in Multiplayer and be joinable without typing an address, switching networks or manually changing Windows routes. Prefer a shared discovery mechanism that works for other games. Verify both hosting and joining on real PCs.
- [x] In the local receive-side experiment, the Minecraft Java 26.2 detector and an independent listener on a random IPv4 multicast group and UDP port each found two overlay announcements and a host-local announcement on Ethernet. The delivery path followed Windows multicast memberships without game-specific addresses, ports, payload parsing or routes. Turning it off hid both overlay announcements. The home source used multicast loopback. Entry into real game worlds and discovery from another physical LAN host are still open.
- [x] Repeat receive-side discovery over the internet with service 129. The stock Minecraft Java 26.2 detector found the remote fixture over EasyTier 2.7.1 on the public UDP mapping. A Java Edition status request to the discovered endpoint returned the matching description and ping nonce. A separate random UDP group and port also worked. The remote fixture simulated the game server. Full gameplay, hosting from a Windows PC, peer-to-peer NAT traversal and simultaneous physical-LAN discovery remain open.

- [x] Connect the invitation service, real virtual IPs, presence and nickname updates. Verified HTTPS invitations, pasting a link, Windows uplink:// registration, approval of a pending node, controller-assigned address, presence and propagation of a Windows nickname to a second API client.
- [x] Check owner permissions on the server and propagate renames. Owners and admins could rename a network and an ordinary member was denied. A second node saw the new name. The previous owner was denied after transferring ownership.
- [x] Measure server and peer latency separately from sample values. For real networks, the local row shows HTTPS /v1/ping time after TLS setup. Windows sends peer ICMP from the selected adapter's IP. Real responses were checked on two /24 networks. Gray bars mark a missing measurement.
- [x] Poll peers on a five-second schedule independent of HTTPS sync. Skip missed ticks and retry a timeout once before clearing the reading. Record each attempt, address pair, duration and Windows error or RTT in the rotating local peer ping log.
- [x] Reuse HTTPS connections across background sync cycles. With three saved memberships, a 50-second capture on the Windows client saw one TCP handshake to instance 129 at startup. A 50-second window starting ten seconds later saw zero new handshakes. A loopback TLS test also covers a server closing the connection between requests.
- [ ] Verify an invitation between two installed Uplink clients. After joining, both should see the new membership and each other's addresses and exchange packets. An installed Windows client and a Linux ZeroTier node using the test API client passed. Two installed Uplink clients remain to be checked.

## Controls and title bar

- [x] The custom title bar shows the Uplink mark and name. Minimize, restore and close to tray were verified.
- [x] Dragging the Slint title bar moves the window. Alt+F4 closes the window to the tray, and clicking the tray icon reopens it.
- [x] An agent can read status, members and invitations as JSON, change the nickname, open and leave networks, select the tray icon and show or hide the window without a mouse.
- [x] Commands return predictable exit codes and machine-readable errors. Changes appear in the open GUI and persist when it is reopened.

## Verification on November 20, 2026

One profile opened one window. Checks covered Windows link handling, copying invitations and addresses, saving nicknames, edits from both GUI and CLI, closing to tray and restoring. The previous layout was checked visually at 464 × 448 at 100% and 696 × 672 at 150%. The 388 × 540 window was checked at 100%. At the time of this run, the server and transport were still pending and the latency values were examples.

Working sets on one PC over three Slint cycles had medians of 24.0 MiB for the window, 36.3 MiB total and 12.3 MiB after closing it. The previous Win32 build with a simpler member view measured 15.3 / 26.3 / 11.0 MiB. Tray main-window lookup was refined after the measurement. Local profiles and measurements are documented in `.temp/README.md`.

Radmin VPN lists members in its main window and exposes ping in each node's context menu. Uplink shows the address and space for latency in the row and accepts invitations. [Radmin VPN help](https://www.radmin-vpn.com/help/)



## Instance 129, November 21–22, 2026

Two ways to join were verified with the installed Windows client. The system link handler opened the first network. For the second, the link was pasted into the main window and confirmed with Join. An invitation was sufficient on the test instance. Each single-use link was valid for one hour and the same session could retry the same request. First join on a new Windows computer required transport setup with the instance's signed root.

TCP, UDP and UDP broadcast passed from Windows to Linux and back, with five of five replies in each of six series. Traffic resumed in each saved network after restarting Uplink. The test with two Windows clients on different physical networks remained open.

At the time of this run, each ZeroTier installation used one private root, real networks used /24 subnets and users joined by link. Server-side avatar sync, QR and password sign-in were pending. Demo-network metadata stayed local.

Reinstalling a ZeroTier 1.16.2 adapter reproducibly affected a neighboring network. With distinct MACs, the neighboring network received 52 of 84 and 33 of 69 replies in two continuous UDP probes. Windows logged updates to both ROOT/NET/0003 and ROOT/NET/0004 during one UpdateDriverForPlugAndPlayDevices call for zttap300. ZeroTier 1.16.2 uses INSTALLFLAG_FORCE when creating an adapter. MAC collisions are handled separately: the API rejects conflicting new IDs with HTTP 409. [WindowsEthernetTap.cpp](https://github.com/zerotier/ZeroTierOne/blob/1.16.2/osdep/WindowsEthernetTap.cpp#L234-L244)

Measurements, configuration snapshots and a screenshot of the actual window are in `.temp/zerotier-129/`. See `.temp/README.md` for descriptions.

## Stopping the transport and retaining adapters, November 22, 2026

- [x] Test targeted installation for the second adapter. With the control service, SetupAPI touched both devices. With the patch, the log showed installation only for the second device and the first kept its GUID. Both runs used the same signed driver, version 3.0.0.1.
- [x] Check UDP on one open socket. The unchanged control delivered 358/358 packets with a maximum reply gap of 92 ms. Recreating the second adapter delivered 355/355 and 357/357, with maximum gaps of 1877 and 1631 ms.
- [x] Test the global switch with the experimental build. Across two cycles of the actual button, the old leave/join path restored both adapters and connectivity in 6.79–10.25 s.
- [x] Test stopping the stock service while retaining devices. In two stop/start cycles with two networks, both adapters went offline and echoes stopped. After restart both GUIDs and PnP IDs persisted and UDP replies resumed 5.85–8.46 s after the start command. Windows logged no driver installations. These were service-control tests. The client button still used leave/join at that point.
Decision on November 22: one persistent Uplink adapter per installation supersedes the earlier per-network adapter plan. The measurements above describe the earlier design.

The target for an explicit adapter state change is about 5 seconds. From enabling to the first peer reply the target is at most 30 seconds. Both still need measurement in the new mode.

In a separate external connectivity test during stock service stop/start, 36/36 TCP connections to 1.1.1.1:443 and 31/36 to the public address of 129 succeeded. While both LANs were off, all 18 external probes succeeded (nine per address). The LANs resumed UDP 7.02 and 8.05 s after the start command. The cause of the five public-address timeouts was not established.

## First join, availability and invitations

- [x] The installer sends version 2 during logical network bootstrap. The PowerShell script was exercised up to its HTTPS call. A clean MSI install is a separate test.
- [x] On HTTPS timeout, read the IP from the live interface and measure known peers again. Clear the list when access is revoked. Verified with a client state test.
- [x] An independent decoder read the full single-use URL from the QR code. Disabling both invitation methods revoked unused invitations issued by the dialog.
- [x] Joining with a server, network name and password was tested separately in the native Slint form and over HTTP with a test controller. Create and rename enforce per-instance name uniqueness. Separate instances can reuse a name.

- [x] Verify a ZeroTier signature on first join and when recovering an old session's privileges. Stock Windows and Linux idtool binaries were tested with temporary keys.
- [x] Retry interrupted joins and revoke partially granted access after a password change. Limit login rate and concurrent password checks. SQLite contention returns HTTP 503 to the client.
- [x] Complete an independent review, fix findings and rerun checks. At that checkpoint, 36 Rust tests passed, as did 18 server tests each on Windows and Linux. Failure and retry paths were checked with PowerShell 5.1 and 7. Windows build `87F21127-80FB3060-6542B3EC` was installed and all three executable hashes were verified with SHA-256.
- [ ] Repeat password sign-in and peer traffic with two installed Windows clients on the new transport.
- [x] Install ZeroTier 1.16.2 on a second Windows PC without a previous ZeroTier service. The MSI ran in SSH session 0 with its headless feature and no reboot. Device proof provisioning resumed after correcting the script encoding for Windows PowerShell 5.1.
- [x] Join the existing Uplink 129 network by invitation from the second Windows PC. Both ZeroTier nodes reported a direct LAN path and exchanged a UDP request and its matching response over their virtual addresses with the remote GUI stopped.
- [ ] Characterize the first-join UDP burst before testing across different physical networks. During one join the two endpoint counters saw 454 packets to and from the provider address, including 320 UDP packets and one new TCP connection. The estimated combined rolling peak was 173 packets/s. A WAN-side measurement is still needed.
- [x] Measure cold joins on one isolated controller interface. With Linux ZeroTier 1.16.2, one existing peer and a client reproducing the Windows API sequence, three fresh joins peaked at 58, 62 and 64 TCP+UDP packets in a rolling second. Waiting locally for the assigned IP before the first sync reduced nine sync/ping pairs to one and produced peaks of 48, 54 and 50. All six clients received a peer echo reply. These measurements cover the isolated Linux setup.
- [x] Wait for a ready local IPv4 in the Rust Windows client after enrollment. On instance 129, one first join to a second network from a prepared Windows device completed in 14.18 s. The instance received one sync and one ping. The joining PC saw 46 TCP and 262 UDP packets to/from the public endpoint, one new TCP connection and one UDP flow already present during baseline. The public HTTPS proxy opened ten backend connections for ten API calls. The temporary membership was removed and the earlier network preserved.
- [x] Send the first invitation redemption immediately and back off after pending enrollment (400 ms, 800 ms, up to 1.6 s within the 25 s retry budget). Rust tests cover the retry schedule and denied invitations. An isolated ZeroTier comparison recorded 12 versus 5 redemption requests and a working peer echo in both runs.
- [ ] Compare the updated Windows client with the previous capture on instance 129 using the same headless peer. Record elapsed join time, API requests, external TCP SYNs, backend connections and peak combined TCP/UDP packets per second.
- [x] Release the SQLite writer while waiting for a device to register and checking its signature. Repeat ticket validation before binding. Local parallel enrollment tests confirmed two independent admissions and a single winner for a shared invitation, including an isolated real ZeroTier controller.


## Joining a network

- [x] Share the dialog frame, header and action sizing across join, invitations, avatar, member confirmations, VPS setup and updates. Join and VPS use the same step indicator. The update waits behind an open flow, and Enter defaults to Cancel in member confirmations. Compare the 16 before and after Slint captures in `.temp/modal-unification/`.
- [x] Give dialog text actions a shared neutral fill or a primary accent. The link form uses a primary icon to join. Opening a member confirmation with the mouse leaves the safe default active without a focus outline. Tab navigation shows the outline.
- [x] Keep dialog actions reachable when a long localized error scrolls. Verify 16 modal states at 125% in Russian and 150% in English, and return keyboard focus to the opener after closing join, release notes, member confirmation and avatar dialogs. Render the crop preview at the window DPI and retain 512 × 512 saved avatars.
- [x] Open a centered join dialog from the network rail or the empty network list. Keep both sign-in methods and connection progress inside the dialog. Restore the entered values on failure.
- [x] Show a copy icon to the right of the selected network's server address on hover. Keep the check visible for two seconds after copying, including when the pointer leaves.
- [x] Network managers can reveal a password they saved on this Windows profile after reopening settings. Protect the local copy with per-user DPAPI and check the server's password revision when available.
- [x] Show progress for an invitation or password join, including links opened from another application. Clear the previous error when retrying and keep the form available after failure.
- [x] Keep all four steps visible after a failed join. Mark the failed step with a cross and its error, allow retry with the same details, and return to the filled form on Back.
- [x] Grant the joining Windows account read access to its verified ZeroTier planet. For an existing setup, repair a read denial once with elevation and verify the file again before enrollment.
- [x] Confirm invitation copying with the same two-second check as IP copying. Let users copy the server address from the network and both the address and network name from password invitations.
- [x] Identify API stages and rejection reasons in the server journal. Correlate ticket issuance and enrollment without exposing the ticket, password, session or device proof.
- [x] Record the join method, timed local operations and available Windows, process, curl and HTTP codes in the Windows join log. Cover link, password and headless joins without logging credentials or raw diagnostic text.
- [x] Join by invitation and password on an isolated controller with two ZeroTier peers and verify that both receive different virtual IPs.
- [ ] Confirm the reported first join on the affected Windows PC. The server journal from before the logging change did not record the failed operation.

## Windows installer

- [x] Center the client and setup window in the active monitor's work area on first display. Both were measured on an isolated Windows desktop.

- [x] The installer compares the client file version with the package version. First install, upgrade from 0.0.9 to 0.1.0 and reinstall of 0.1.0 were checked in the GUI.
- [x] The NSIS package Uplink-Setup.exe uses a Slint window with the app's icon, font and buttons. The version appears below the title.
- [x] The folder icon opens the system directory picker. Install, relocation, reinstall and uninstall were tested with a path containing Cyrillic and spaces.
- [x] Verified the Start menu shortcut, invite registration, migration of start-at-sign-in settings and preservation of an unrelated file in the install directory during uninstall.
- [x] The completion button launches the registered client. The installer remembers the selected client folder for later runs, including silent updates. Verified a custom path with spaces and Cyrillic, a repeat installation and cleanup. The network service runs from the protected machine package.
- [x] Rollback uses a short label. Closing the client needs separate consent. Uninstall offers independent choices for user data and ZeroTier.
