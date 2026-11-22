# Release approval

Write release notes in English for people who use Uplink. Group changes by what users can do, explain any action they need to take, and mention limitations that affect the release. Keep draft, tag, package version, signed notes, and the installer together.

## Changelog format

Starting with 0.1.2, maintain one file per version in `release/notes/`. Add an entry when a change is ready to ship. The GitHub release and the in-app update dialog use the same approved text from this file.

Group entries by where people will notice the change. Reuse these level-two headings across releases: **🌐 Networks** for joining, invitations and connections, **🖥️ Interface** for shared window and tray controls, **🔄 Updates** for discovering, reading about and installing updates, and **🛠️ For server owners** for running an instance. Use only the areas affected, with the most important one first. A new area can get its own heading when it helps readers find an entry.

Give each outcome one home. A visual fix specific to the update dialog belongs under Updates. A change to focus indicators throughout the app belongs under Interface. New features and fixes can share a section: start with what people can now do or name the problem that was corrected. Call out any required action or security impact in the relevant bullet.

Write one short bullet per outcome. Combine related commits, and keep plans in the roadmap. GitHub already displays the version and publication date.

## Prepare a release

After pushing the reviewed sources, run `./release/draft.ps1 -Version X.Y.Z` on the Windows publisher account. This tests the client, builds the NSIS installer, signs the release manifest using the publisher’s Windows CNG key, and uploads a GitHub **draft**. Client devices only need the embedded public key.

Show the owner the draft link, exact English notes, version, target commit and installer SHA-256. Obtain explicit approval **for each release** before publishing. A change to the package or the notes requires a new review and signature. No push, tag, build or workflow publishes a release automatically.

The original publisher key resides in the current user's Microsoft Software Key Storage Provider under `VoyagerVPN.Uplink.Release.v1`. It is non-exportable. Before retiring this Windows profile, ship a client that trusts a replacement key while the current key is still available.
