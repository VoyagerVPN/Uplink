# Uplink UX checklist

Checked items record verified behavior. The early UX checks used the Windows client with a local network model. Later sections record tests with the live transport.

## Release plan

- **0.1.0** is the published Windows installer for joining existing instances.
- **0.1.1** covers the current client, transport and server fixes, including HTTPS connection reuse during background sync.
- **0.1.2** gives release notes a compact scrollbar with a dedicated gutter.
- **0.2.0** covers VPS setup from the Windows UI, from entering SSH credentials through creating the first network and receiving its invitation.

## Port of the approved HTML prototype to Rust

- [x] At 388 × 540, the network rail starts at (20, 116). The first icon is 32 × 32 and icons are spaced 41 px apart. The plus and the invitation field's close icon are centered in the same column.
- [x] The selected network name and server address start at x = 75. Table headers start at y = 183 and the 32 px rows start at y = 209. The title-bar sliders open app settings.
- [x] Invite links open all five HTML demo networks. Select a saved network in the rail to switch views.
- [x] The network gear opens Overview, Invitations and Members. Each test network stores its own changes.
- [x] Choose an avatar with the Windows file picker or drop an image. The editor shows a rounded preview and supports panning, zooming and replacing the source. It saves a 512 × 512 square.
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
- [ ] Manually check contrast on a light taskbar and the colors for good, moderate and high measured latency with the live transport. On a dark taskbar, the green and Windows-theme icons were legible in the Explorer overflow. Demo ping mode displayed green bars for the 38 ms sample.
- [x] Components and layouts can be edited directly. The library choice was evaluated with a real window displaying Cyrillic and a memory measurement.
- [x] Compare the window and total working sets with the Win32 client while viewing a network and after closing the window.

## Terminology

- [x] The UI calls the membership space a “network”. “Server” refers to the endpoint for the server ping.

## Multiple networks

- [x] Saved memberships are independent of power. When off, the area below the profile centers a muted outline of an unplugged cable and the label “Uplink is off”. Only the cable and label brighten on hover. Clicking restores the full list and the last selected network.
- [x] My Networks is accessible from the main screen. Each entry shows the name and server. One click opens the members. Switching preserves the order. Join opens and focuses the invite field. Two local networks use distinct IPs, member lists and links. Reopening an invitation selects the existing network. Leave removes only the selected membership.
- [x] The settings gear uses a square icon area. Switches use the Uplink accent and the same 6 px radius as buttons. Hovering over the track or thumb highlights it and shows a hand cursor.
- [ ] Check saved networks on different Uplink instances, including instances with different private roots.

Multiple Uplink networks can be active simultaneously. Selecting a network in the rail opens its members while keeping connections active. On one instance, groups share a ZeroTier transport with membership isolation. Peers without a shared network must receive neither direct packets nor LAN announcements from one another.

- [x] Three Linux clients running ZeroTier 1.16.2 shared one transport, with one peer in groups A and B. Direct UDP and LAN announcements passed in both directions on one interface only between peers in a shared group. Removing and restoring group B membership preserved the common peer's interface. Clients used the private planet from instance 129 and a separate local controller.
- [x] The API and Windows client distinguish logical networks from the shared transport. Two logical networks were deployed on instance 129. Server checks cover invite joins, member lists, management and revocation of an individual membership. The installed Windows client supports both network formats.
- [x] On the test Windows installation, the standard CLI disabled the shared adapter in 1.02 s and enabled it in 0.83–0.89 s with the same GUID. Two previously created interfaces stayed enabled. These timings measure the device state change.
- [ ] Verify the installed window and CLI against instance 129: check both lists, a stable GUID and uninterrupted UDP in A when leaving and rejoining B. At 01:23 Moscow time on November 22, public HTTPS and SSH on 129 did not respond and Windows did not receive the shared transport configuration.

## Adapter lifecycle

- [ ] Implement one persistent virtual Uplink adapter per Windows installation. On clean Windows, verify that power cycles, network changes and restarts preserve the name and GUID without creating additional devices.
Earlier results with one adapter per network:

- [x] Repeat power cycles and client restarts with one and two networks. Each active network had its own adapter. Two power cycles and a restart in each configuration preserved interface names without accumulating devices. Windows showed zero ZeroTier adapters when off and one or two when on. Recreating a device changed its GUID.
- [x] Measure from clicking Enable Uplink to the first successful packet exchange with a peer's virtual IP. The target was 30 s including adapter creation and address assignment. From the installed Windows button to the first Linux UDP response, one network took 5.04 and 7.25 s and two networks took 8.32–10.88 s.

## Server and transport

- [x] Generate a private Linux root from the supplied UDP endpoint with the stock ZeroTier 1.16.2 binary. Confirm repeated setup preserves the keys and signed planet and refuses an incomplete bundle or endpoint change.

- [x] On a fresh isolated Linux instance with ZeroTier 1.16.2, create two logical networks on the owner socket by name alone. Verify a single shared transport, idempotent retries, duplicate-name rejection, bootstrap of an invitation against the generated private planet, and rejection of network creation on the public client API.
- [x] Run the Windows installer silently in an SSH service session on a second physical PC. Verify a stopped window and exercise the development CLI's headless join with an isolated demo profile. This checks the unattended UI path on one LAN. The live two-client test below remains open.

- [ ] Let the owner create their first network on a rented VPS from the Windows client. After the owner grants access, a single action installs the Uplink server, creates the network and returns an invitation for friends. Subsequent networks reuse the installed server.

Uplink's test instance is ServerHQ service 129.

Only the instance owner can create networks on that instance. They become the first network owner and can transfer ownership to a member. The transfer applies only to that network. A regular invitation grants the member role.

- [x] Check permissions through the server API. On 129 the instance owner created a private network through the Unix socket. A member, an admin and the new network owner after a transfer each received HTTP 403 on a PUT to create another network. The controller confirmed that the requested network had not been created.

Uplink is for private networks among friends. The project aims to publish the Windows client and server source. The owner hosts the server and members manage networks from the Uplink GUI.

- [x] On service 129, verify UDP forwarding to the private root, start the private controller and create a private network through the Uplink API. Address assignment was denied before approval, succeeded after approval and the network survived a service restart.
- [x] Check initial peer discovery without public ZeroTier roots. Two fresh Linux clients in separate Docker networks started with the signed `planet`. Instance 129 supplied the only external IPv4 route. Neither client had an external IPv6 route.
- [x] Test an established connection between two Linux clients. TCP, UDP and UDP broadcast passed both ways over the virtual interfaces. All 30 messages arrived in the final run via instance 129. Results immediately after joining and after restarting were also recorded.
- [ ] Measure broadcast readiness immediately after joining. On the first run in a new network, all five packets were lost in one direction. On retry, all five arrived in each direction. Over three Linux rejoins, the first broadcasts in both directions arrived 0.45–3.31 s after the timestamp taken before join. The upper bound and reason for the initial loss remain open.
- [ ] Repeat joining, bidirectional TCP and UDP, UDP broadcast and latency measurement with two clean Windows Uplink clients on different physical networks. Record whether the route is direct or relayed.

- [x] Connect the invitation service, real virtual IPs, presence and nickname updates. Verified HTTPS invitations, pasting a link, Windows uplink:// registration, approval of a pending node, controller-assigned address, presence and propagation of a Windows nickname to a second API client.
- [x] Check owner permissions on the server and propagate renames. Owners and admins could rename a network and an ordinary member was denied. A second node saw the new name. The previous owner was denied after transferring ownership.
- [x] Measure server and peer latency separately from sample values. For real networks, the local row shows HTTPS /v1/ping time after TLS setup. Windows sends peer ICMP from the selected adapter's IP. Real responses were checked on two /24 networks. Gray bars mark a missing measurement.
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
- [ ] Update instance 129, repeat password sign-in on two installed Windows clients, verify migration of the previous owner and exchange packets.
- [x] Install ZeroTier 1.16.2 on a second Windows PC without a previous ZeroTier service. The MSI ran in SSH session 0 with its headless feature and no reboot. Device proof provisioning resumed after correcting the script encoding for Windows PowerShell 5.1.
- [x] Join the existing Uplink 129 network by invitation from the second Windows PC. Both ZeroTier nodes reported a direct LAN path and exchanged a UDP request and its matching response over their virtual addresses with the remote GUI stopped.
- [ ] Characterize the first-join UDP burst before testing across different physical networks. During one join the two endpoint counters saw 454 packets to and from the provider address, including 320 UDP packets and one new TCP connection. The estimated combined rolling peak was 173 packets/s. A WAN-side measurement is still needed.
- [x] Measure cold joins on one isolated controller interface. With Linux ZeroTier 1.16.2, one existing peer and a client reproducing the Windows API sequence, three fresh joins peaked at 58, 62 and 64 TCP+UDP packets in a rolling second. Waiting locally for the assigned IP before the first sync reduced nine sync/ping pairs to one and produced peaks of 48, 54 and 50. All six clients received a peer echo reply. These measurements cover the isolated Linux setup.
- [x] Wait for a ready local IPv4 in the Rust Windows client after enrollment. On instance 129, one first join to a second network from a prepared Windows device completed in 14.18 s. The instance received one sync and one ping. The joining PC saw 46 TCP and 262 UDP packets to/from the public endpoint, one new TCP connection and one UDP flow already present during baseline. The public HTTPS proxy opened ten backend connections for ten API calls. The temporary membership was removed and the earlier network preserved.
- [x] Send the first invitation redemption immediately and back off after pending enrollment (400 ms, 800 ms, up to 1.6 s within the 25 s retry budget). Rust tests cover the retry schedule and denied invitations. An isolated ZeroTier comparison recorded 12 versus 5 redemption requests and a working peer echo in both runs.
- [ ] Compare the updated Windows client with the previous capture on instance 129 using the same headless peer. Record elapsed join time, API requests, external TCP SYNs, backend connections and peak combined TCP/UDP packets per second.
- [x] Release the SQLite writer while waiting for a device to register and checking its signature. Repeat ticket validation before binding. Local parallel enrollment tests confirmed two independent admissions and a single winner for a shared invitation, including an isolated real ZeroTier controller.
- [ ] If final controller writes are slow in practice, coordinate authorization with revocation and shared transport tag updates before moving those writes outside the SQLite transaction.
- [ ] Verify an uninterrupted first-time MSI installation and device proof provisioning on another clean Windows system.


## Windows installer

- [x] The installer compares the client file version with the package version. First install, upgrade from 0.0.9 to 0.1.0 and reinstall of 0.1.0 were checked in the GUI.
- [x] The NSIS package Uplink-Setup.exe uses a Slint window with the app's icon, font and buttons. The version appears below the title.
- [x] The folder icon opens the system directory picker. Install, relocation, reinstall and uninstall were tested with a path containing Cyrillic and spaces.
- [x] Verified the Start menu shortcut, invite registration, migration of start-at-sign-in settings and preservation of an unrelated file in the install directory during uninstall.
- [x] The completion screen offers to open Uplink. The first network join starts ZeroTier setup from the client.
- [x] Rollback uses a short label. Closing the client needs separate consent. Uninstall offers independent choices for user data and ZeroTier.
