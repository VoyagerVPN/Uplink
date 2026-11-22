param([Parameter(Position=0)][ValidateSet('build','test','run','perf','fmt','fetch')][string]$Task='run', [Parameter(ValueFromRemainingArguments=$true)][string[]]$AppArgs)
$ErrorActionPreference = 'Stop'
Set-Location -LiteralPath $PSScriptRoot
$env:CARGO_HOME = Join-Path $PSScriptRoot '.cache/cargo'
$env:CARGO_TARGET_DIR = Join-Path $PSScriptRoot '.cache/target'
$localRust = Join-Path $PSScriptRoot '.cache/rustup'
if (Test-Path $localRust) { $env:RUSTUP_HOME = $localRust }
$env:PATH = (Join-Path $env:CARGO_HOME 'bin') + ';' + $env:PATH
if (-not (Get-Command cargo -ErrorAction SilentlyContinue)) {
    $env:RUSTUP_HOME = $localRust
    $installer = Join-Path $PSScriptRoot '.cache/rustup-init.exe'
    $uri = 'https://static.rust-lang.org/rustup/dist/x86_64-pc-windows-msvc/rustup-init.exe'
    New-Item -Type Directory -Force (Split-Path $installer) | Out-Null
    Invoke-WebRequest -UseBasicParsing $uri -OutFile $installer
    $checksum = (Invoke-WebRequest -UseBasicParsing "$uri.sha256").Content
    if ($checksum -is [byte[]]) { $checksum = [Text.Encoding]::ASCII.GetString($checksum) }
    $expected = ($checksum.Trim() -split '\s+')[0]
    if ((Get-FileHash -LiteralPath $installer).Hash -ne $expected) { throw 'Rust installer checksum mismatch' }
    & $installer -y --no-modify-path --profile minimal --component rustfmt --default-toolchain 1.100.0
    if ($LASTEXITCODE) { exit $LASTEXITCODE }
}
switch ($Task) {
    'run'   {
        cargo build --locked --bins
        if ($LASTEXITCODE) { exit $LASTEXITCODE }
        cargo run --locked --bin uplink -- @AppArgs
    }
    'perf'  { cargo build --locked --profile perf }
    'build' { cargo build --locked }
    'test'  { cargo test --locked }
    'fmt'   { cargo fmt --all }
    'fetch' { cargo fetch }
}
exit $LASTEXITCODE
