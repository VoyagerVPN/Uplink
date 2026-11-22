# Release approval

Write release notes in English for people who use Uplink. Group changes by what users can do, explain any action they need to take, and mention limitations that affect the release. Keep draft, tag, package version, signed notes, and the installer together.

## Changelog format

Starting with 0.1.2, maintain one file per version in `release/notes/`. Add an entry when a change is ready to ship. The GitHub release and the in-app update dialog use the same approved text from this file.

Group entries by where people will notice the change. Use `## 🖥️ UI` for windows, dialogs and tray controls, `## 🌐 Net` for joining, invitations and connections, and `## 🛠️ For server owners` for operating an instance. Include only the areas affected, most important first.

Start each entry with **✦ New** for a new capability, **↻ Changed** for a change in existing behavior, or **✓ Fixed** for a resolved problem. Follow the label with an em dash and one short sentence. Put each entry on one line with a blank line between entries. The symbol serves as its bullet in both GitHub and the app.

Give each outcome one entry. Mention any required action, replacement for a removed feature, or security impact in that entry. Keep plans in the roadmap. GitHub already displays the version and publication date.

## Prepare a release

After pushing the reviewed sources, run `./release/draft.ps1 -Version X.Y.Z` on the Windows publisher account. This tests the client, builds the NSIS installer, signs the release manifest using the publisher’s Windows CNG key, and uploads a GitHub **draft**. Client devices only need the embedded public key.

Show the owner the draft link, exact English notes, version, target commit and installer SHA-256. Obtain explicit approval **for each release** before publishing. A change to the package or the notes requires a new review and signature. No push, tag, build or workflow publishes a release automatically.

The original publisher key resides in the current user's Microsoft Software Key Storage Provider under `VoyagerVPN.Uplink.Release.v1`. It is non-exportable. Before retiring this Windows profile, ship a client that trusts a replacement key while the current key is still available.
