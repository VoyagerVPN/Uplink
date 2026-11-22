# Release approval

Write release notes in English for people who use Uplink. Group changes by what users can do, explain any action they need to take, and mention limitations that affect the release. Keep draft, tag, package version, signed notes, and the installer together.

After pushing the reviewed sources, run `./release/draft.ps1 -Version X.Y.Z` on the Windows publisher account. This tests the client, builds the NSIS installer, signs the release manifest using the publisher’s Windows CNG key, and uploads a GitHub **draft**. Client devices only need the embedded public key.

Show the owner the draft link, exact English notes, version, target commit and installer SHA-256. Obtain explicit approval **for each release** before publishing. A change to the package or the notes requires a new review and signature. No push, tag, build or workflow publishes a release automatically.

The original publisher key resides in the current user's Microsoft Software Key Storage Provider under `VoyagerVPN.Uplink.Release.v1`. It is non-exportable. Before retiring this Windows profile, ship a client that trusts a replacement key while the current key is still available.
