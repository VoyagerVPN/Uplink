param([Parameter(Mandatory)][string]$Cache)
$ErrorActionPreference = 'Stop'
$version = '2.7.1'
$artifact = "easytier-windows-x86_64-v$version.zip"
$archive = Join-Path $Cache 'windows.zip'
$digest = '6279CE18906F345345780B2F0C507125FCAE861A54C45CC62EFE58D5312DE46B'
New-Item -ItemType Directory -Force $Cache | Out-Null
if (!(Test-Path -LiteralPath $archive)) {
    $download = "$archive.$([guid]::NewGuid().ToString('N')).tmp"
    try {
        Invoke-WebRequest -UseBasicParsing "https://github.com/EasyTier/EasyTier/releases/download/v$version/$artifact" -OutFile $download
        if ((Get-FileHash -LiteralPath $download).Hash -ne $digest) { throw 'Unexpected EasyTier archive digest' }
        [IO.File]::Move($download, $archive)
    } finally { if (Test-Path -LiteralPath $download) { [IO.File]::Delete($download) } }
}
if ((Get-FileHash -LiteralPath $archive).Hash -ne $digest) { throw 'Unexpected EasyTier archive digest' }
$dest = Join-Path $Cache 'package-input'
$bin=Join-Path $dest 'easytier-windows-x86_64'
. (Join-Path $PSScriptRoot 'package-files.ps1')
Copy-ArchiveFiles $archive @{
    'easytier-windows-x86_64/Packet.dll'='Packet.dll'
    'easytier-windows-x86_64/wintun.dll'='wintun.dll'
    'easytier-windows-x86_64/WinDivert64.sys'='WinDivert64.sys'
} $bin
& (Join-Path $PSScriptRoot 'build-engine.ps1') -Cache $Cache -Output (Join-Path $bin 'uplink-engine.exe')
$bin
