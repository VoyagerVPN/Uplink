# Release approval

Write release notes in English for people who use Uplink. Group changes by what users can do, explain any action they need to take, and mention limitations that affect the release. Keep draft, tag, package version, signed notes, and the installer together.

## Changelog format

Starting with 0.1.2, maintain one file per version in `release/notes/`. Add an entry when a change is ready to ship. The GitHub release and the in-app update dialog use the same approved text from this file.

Use these level-two headings in order, including only categories with entries. This adapts [Keep a Changelog](https://keepachangelog.com/en/1.1.0/) for the update dialog.

| Heading | What belongs here |
| --- | --- |
| ✨ Added | Something a user can now do |
| 🔄 Changed | A change to how an existing feature works |
| ⏳ Deprecated | A feature scheduled for removal and what to use instead |
| 🗑️ Removed | A feature that is gone and any action users need to take |
| 🐛 Fixed | A problem users could encounter and the corrected behavior |
| 🔒 Security | A security issue addressed by this update and any action required |

Use one short bullet per user-visible outcome. Combine related commits and leave out merges, internal refactors and routine maintenance. If a change affects only people running a server, begin its bullet with “For server owners”. Keep plans in the roadmap. GitHub already displays the version and publication date.

After pushing the reviewed sources, run `./release/draft.ps1 -Version X.Y.Z` on the Windows publisher account. This tests the client, builds the NSIS installer, signs the release manifest using the publisher’s Windows CNG key, and uploads a GitHub **draft**. Client devices only need the embedded public key.

Show the owner the draft link, exact English notes, version, target commit and installer SHA-256. Obtain explicit approval **for each release** before publishing. A change to the package or the notes requires a new review and signature. No push, tag, build or workflow publishes a release automatically.

The original publisher key resides in the current user's Microsoft Software Key Storage Provider under `VoyagerVPN.Uplink.Release.v1`. It is non-exportable. Before retiring this Windows profile, ship a client that trusts a replacement key while the current key is still available.
