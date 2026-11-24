# Uplink

[Russian README](README.ru.md)

Uplink connects Windows PCs to a self-hosted virtual LAN. The desktop client uses Rust and Slint 1.19 with software rendering. The [UX checklist](UX-CHECKLIST.md) tracks verified behavior and outstanding work.

## Join a self-hosted instance

Install the Windows package with an administrator account, then open Uplink as your regular user and paste the invitation. To host a network, choose **Join network → Create a network on my VPS**. Use a Debian 12 or Ubuntu 24.04 x86-64 VPS with public IPv4, SSH access, TCP 80/443 and UDP 9993. Uplink remembers the SSH key on first contact and stops if it changes. If the VPS uses a provider HTTPS proxy, select that option on the confirmation screen and enter the proxy hostname, private VPS address and public UDP port. [Server guide](server/README.md)

The VPS assigns a private IPv4 address and a personal EasyTier credential to each member. Uplink exchanges nicknames and membership changes over HTTPS. The Windows service connects independent instances through one Uplink adapter. Each network keeps its own IPv4 address. Joining or leaving one network preserves the other connections. The power button stops or restores the saved connections. Membership lasts until you leave the network or are removed. After a long absence, Uplink uses your saved session to renew the device credential and reconnect with the same IP address.

## Try the interface with demo networks

Once Uplink is registered, open `uplink://join/after-hours` or paste the link into the invitation field. **Invite** shows the methods enabled for the selected network. Owners and admins can enable links, QR codes and sign-in with a server address, network name and password. Your nickname defaults to your Windows username. Hover over it and click the pencil to edit. Enter or a click outside saves it. Escape cancels. Hover over an IP address and click to copy it. A check mark appears for two seconds.

The table has one row per member. Click **Ping** or **Members** to sort. A crown marks the owner. You own the demo network opened by `uplink://join/after-hours`. Owners and admins can open network settings with the gear icon. In **Overview**, use the pencil to rename the network or click the avatar or drop a photo to crop it. The saved image is 512 × 512. Enter or a click outside saves the name. Escape cancels. Existing invites still work after a rename.

**Leave** removes you from the selected network. Closing the window releases its memory. Use the tray icon or launch Uplink again to reopen it. The tray menu offers a green icon, a ping indicator or a color that follows the Windows theme.

That demo network uses sample members and addresses. Its example latencies are 38 ms to the server and 24, 93 and 186 ms to three peers. Three gray bars indicate a missing measurement. Hover over the bars to see which endpoint the value refers to. Invitation links open on computers with Uplink registered.

Saved networks appear on the left. Click an icon to see the members and server address. A green outline marks the selection. The plus icon opens the invitation field. Try `uplink://join/after-hours`, `uplink://coop.example/rooms/coop`, `uplink://join/arena`, `uplink://join/pine` or `uplink://join/night`. Reopening an invitation selects the saved network.

## Power and settings

The power button next to your nickname controls all networks. In the prototype, turning it off pauses local connections and keeps saved networks, the window and the tray available. Your virtual IP appears in the selected network's table. Green means normal operation, gray means off, orange signals a connection problem and red signals a critical error. Hover for details.

Open app settings with the sliders in the title bar. **Start at sign-in** launches Uplink in the tray for the current Windows user. **Auto-connect** restores the last power state at startup. With it disabled, turn Uplink on manually after launch. Reopening the window from the tray resumes the current session. Click the version in the title bar to check update status. With automatic updates enabled, the tray checks GitHub Releases at startup and every six hours. Once Uplink verifies the release signature and package SHA-256, you can read the notes and choose **Install and restart**.

To preview updates, run `./dev.ps1 build`, then `.cache/target/debug/uplink-window.exe --preview-update found`. The preview uses a temporary profile and lets you open release notes, download and install a test update. Replace `found` with `loading`, `verifying`, `ready` or `offline` to preview a specific state.

## Interface languages

Select a language in Settings. Uplink saves it in your profile. Russian is the default, and English is included. Members choose their own nicknames and network names.

To add a translation, copy the [Russian catalog](lang/ru/LC_MESSAGES/uplink.po) to `lang/<code>/LC_MESSAGES/uplink.po`, set `Language` and `X-Display-Name` in the header, translate the `msgstr` entries and rebuild. The new language appears in Settings. Source strings are in English. Entries with the `native` context cover the tray and messages from Rust.

## Command-line control

`uplinkctl.exe` is installed alongside `uplink.exe` and `uplink-window.exe`. Run `uplinkctl --help` for available commands. Successful commands write JSON to stdout. Errors go to stderr as JSON and return a nonzero exit code.

```powershell
.\uplinkctl.exe status
.\uplinkctl.exe language en
.\uplinkctl.exe language ru
.\uplinkctl.exe power off
.\uplinkctl.exe power on
.\uplinkctl.exe setting autoconnect on
.\uplinkctl.exe demo-health warning
.\uplinkctl.exe demo-health ok
.\uplinkctl.exe join uplink://join/after-hours
.\uplinkctl.exe rooms
.\uplinkctl.exe select uplink://join/after-hours
.\uplinkctl.exe members
.\uplinkctl.exe rename-room "Friday Night"
.\uplinkctl.exe invite
.\uplinkctl.exe nick "Grai"
.\uplinkctl.exe tray system
.\uplinkctl.exe leave
.\uplinkctl.exe hide
.\uplinkctl.exe show
```

For an unattended join, run `uplinkctl join --headless URL` after installing the Windows package. The CLI and desktop app use the same service and enrollment flow.

`window` can be `open`, `minimized`, `tray` or `stopped`. The selected network's `source` is `easytier` or `local_fixture`. An unknown ping is `null`. Tray modes are `green`, `ping` and `system`. You can read state and edit the profile while the window is closed. Profile changes notify the open GUI. The `show` and `join` commands open the window.

## Build and install on Windows

Use Windows x64 with Visual Studio Build Tools, MSVC and the Windows SDK. Run PowerShell from the project root.

```powershell
.\dev.ps1 test
.\installer\build.ps1
.\.cache\packages\Uplink-Setup.exe
```

Build the graphical installer with `.\installer\build.ps1`. The output is `.cache/packages/Uplink-Setup.exe`. The script downloads NSIS 3.13 into the project cache and verifies the archive's SHA-256. For a silent install, run `Uplink-Setup.exe /S`. Uninstall from Windows Settings. The uninstaller keeps your profile and saved networks by default. Select the profile checkbox to remove them.

Setup installs the shared Windows network service and registers invitation links. For development, run `.\dev.ps1 run`. Settings are stored in `%APPDATA%\Uplink\prefs.json`. Set `UPLINK_PREFS_DIR` for both CLI and GUI processes to use an isolated profile.

The Inter license is included in [assets/fonts/Inter-LICENSE.txt](assets/fonts/Inter-LICENSE.txt). For public distribution, select a Slint license and meet its attribution requirements. [Slint terms](https://slint.dev/terms-and-conditions)

## Repository language

Use English for documentation, issues, pull requests and release notes. Write commit messages in English using Conventional Commits. Keep the optional Russian README in sync with this file.
