param([Parameter(Mandatory)][string]$Cache, [Parameter(Mandatory)][string]$Output)
$ErrorActionPreference='Stop'
$repo=Split-Path -Parent $PSScriptRoot
function Get-Archive([string]$File,[string]$Uri,[string]$Hash) {
    $path=Join-Path $Cache $File
    if (!(Test-Path -LiteralPath $path)) {
        $temp="$path.$([Guid]::NewGuid().ToString('N')).tmp"
        try {
            Invoke-WebRequest -UseBasicParsing $Uri -OutFile $temp
            if ((Get-FileHash -LiteralPath $temp).Hash -ne $Hash) { throw "Invalid checksum for $File" }
            [IO.File]::Move($temp,$path)
        } finally { if (Test-Path -LiteralPath $temp) { [IO.File]::Delete($temp) } }
    }
    if ((Get-FileHash -LiteralPath $path).Hash -ne $Hash) { throw "Invalid checksum for $File" }
    return $path
}
$archive=Get-Archive 'repo.tar.gz' 'https://codeload.github.com/EasyTier/EasyTier/tar.gz/refs/tags/v2.7.1' 'DD0BA6B726A843C3C25D2DCDE4CAC1002738C2246D70D647747C64ADF21FC7B8'
$tools=Get-Archive 'protoc-26.zip' 'https://github.com/protocolbuffers/protobuf/releases/download/v26.0-rc1/protoc-26.0-rc-1-win64.zip' '61D80FDD57B9EA1072625085CBAD085DD00310652EA494B8E38C6B76C9EE0516'
$divert=Get-Archive 'divert-2.2.2.zip' 'https://github.com/basil00/WinDivert/releases/download/v2.2.2/WinDivert-2.2.2-A.zip' '63CB41763BB4B20F600B6DE04E991A9C2BE73279E317D4D82F237B150C5F3F15'
$bin=Split-Path -Parent $Output
. (Join-Path $PSScriptRoot 'package-files.ps1')
Copy-ArchiveFiles $divert @{
    'WinDivert-2.2.2-A/x64/WinDivert.dll'='WinDivert.dll'
    'WinDivert-2.2.2-A/LICENSE'='WinDivert-LICENSE.txt'
} $bin
$source=Join-Path $Cache 'engine-src'
$upstream=Join-Path $source 'EasyTier-2.7.1'
if (!(Test-Path -LiteralPath "$source/.extracted")) {
    New-Item -ItemType Directory -Force $source | Out-Null
    & tar -xzf $archive -C $source
    if ($LASTEXITCODE) { throw 'Cannot extract EasyTier sources' }
    New-Item -ItemType File "$source/.extracted" -Force | Out-Null
}
$dest=Join-Path $upstream 'easytier/src/bin'
New-Item -ItemType Directory -Force $dest | Out-Null
$entry=Join-Path $repo 'transport/windows/uplink-engine.rs'
if (!(Test-Path "$dest/uplink-engine.rs") -or (Get-FileHash $entry).Hash -ne (Get-FileHash "$dest/uplink-engine.rs").Hash) {
    Copy-Item -LiteralPath $entry -Destination $dest -Force
}
$env:CARGO_HOME=Join-Path $repo '.cache/cargo'
$env:CARGO_TARGET_DIR=Join-Path $repo '.cache/et-target'
if (Test-Path "$repo/.cache/rustup") { $env:RUSTUP_HOME=Join-Path $repo '.cache/rustup' }
$env:PATH=(Join-Path $env:CARGO_HOME 'bin')+';'+$env:PATH
$env:PROTOC=Join-Path $Cache 'protoc/bin/protoc.exe'
$env:PROTOC_INCLUDE=Join-Path $Cache 'protoc/include'
if (!(Test-Path $env:PROTOC)) { Expand-Archive -LiteralPath $tools -DestinationPath "$Cache/protoc" -Force }
& cargo build --release --locked --manifest-path "$upstream/Cargo.toml" -p easytier --bin uplink-engine --no-default-features --features 'tun,management,ring-crypto,tcp-hole-punch,easytier-core/proxy-packet'
if ($LASTEXITCODE) { throw 'Uplink network engine build failed' }
Copy-Item -LiteralPath "$env:CARGO_TARGET_DIR/release/uplink-engine.exe" -Destination $Output -Force
