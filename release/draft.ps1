param([Parameter(Mandatory)][ValidatePattern('^\d+\.\d+\.\d+$')][string]$Version)
$ErrorActionPreference = 'Stop'
$repo = 'VoyagerVPN/Uplink'
Push-Location (Split-Path $PSScriptRoot)
try {
    if (git status --porcelain) { throw 'Commit the reviewed sources before building a release.' }
    $sha = git rev-parse HEAD
    $branch = git branch --show-current
    $remote = git ls-remote origin "refs/heads/$branch"
    if ($LASTEXITCODE -or !$remote -or $remote.Split()[0] -ne $sha) {throw 'Push the reviewed commit before preparing a draft.'}
    $declared=[regex]::Match((Get-Content Cargo.toml -Raw),'(?m)^version = "(\d+\.\d+\.\d+)"').Groups[1].Value
    if ($Version -cne $declared) {throw 'Cargo version does not match the requested tag.'}
    $notes = (Resolve-Path "release/notes/v$Version.md").Path
    & ./dev.ps1 test
    if ($LASTEXITCODE) {throw 'Client tests failed.'}
    & ./installer/build.ps1
    if ($LASTEXITCODE) {throw 'Installer build failed.'}
    $setup = (Resolve-Path .cache/packages/Uplink-Setup.exe).Path
    $manifest = Join-Path (Split-Path $setup) 'uplink-release.json'
    & ./release/sign.ps1 -Setup $setup -Client .cache/target/perf/uplink.exe -Version $Version -Notes $notes -Output $manifest
    $env:CARGO_HOME=Join-Path $PWD '.cache/cargo'
    $env:CARGO_TARGET_DIR=Join-Path $PWD '.cache/target'
    if (Test-Path .cache/rustup) {$env:RUSTUP_HOME=Join-Path $PWD '.cache/rustup'}
    $env:PATH=(Join-Path $env:CARGO_HOME 'bin')+';'+$env:PATH
    cargo test --locked --lib publisher_manifest_matches_built_windows_package -- --ignored
    if ($LASTEXITCODE) {throw 'Signed release verification failed.'}
    gh release create "v$Version" $setup $manifest --repo $repo --draft --target $sha --title "Uplink $Version" --notes-file $notes
    if ($LASTEXITCODE) {throw 'Could not create the GitHub release draft.'}
    Get-FileHash -LiteralPath $setup,$manifest | Format-Table -AutoSize
} finally {Pop-Location}
