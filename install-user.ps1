param([Parameter(Mandatory)][string]$SourceDirectory)
$ErrorActionPreference = 'Stop'
$source = (Resolve-Path -LiteralPath $SourceDirectory).Path
$files = 'uplink.exe','uplink-window.exe','uplinkctl.exe'
$hashes = foreach ($file in $files) { (Get-FileHash -LiteralPath (Join-Path $source $file)).Hash }
$tag = ($hashes | ForEach-Object { $_.Substring(0,8) }) -join '-'
$base = Join-Path $env:LOCALAPPDATA 'Programs/Uplink'
$install = Join-Path $base $tag
$command = '"' + (Join-Path $install 'uplink.exe') + '" "%1"'
$key = 'HKCU:\Software\Classes\uplink'
$prior = Get-Item -LiteralPath 'Registry::HKEY_CLASSES_ROOT\uplink' -ErrorAction SilentlyContinue
if ($prior -and $prior.GetValue('') -ne 'URL:Uplink Invitation') {
    throw 'Протокол uplink уже принадлежит другому приложению'
}
New-Item -ItemType Directory -Path $install -Force | Out-Null
for ($i = 0; $i -lt $files.Count; $i++) {
    $file = $files[$i]
    $target = Join-Path $install $file
    if (!(Test-Path -LiteralPath $target)) { Copy-Item -LiteralPath (Join-Path $source $file) -Destination $target }
    if ((Get-FileHash -LiteralPath $target).Hash -ne $hashes[$i]) { throw "Повреждена сборка $file" }
}
Copy-Item -LiteralPath (Join-Path $PSScriptRoot 'assets/fonts/Inter-LICENSE.txt') -Destination $install -Force
New-Item -Path "$key\shell\open\command" -Force | Out-Null
Set-Item -LiteralPath $key -Value 'URL:Uplink Invitation'
New-ItemProperty -LiteralPath $key -Name 'URL Protocol' -Value '' -PropertyType String -Force | Out-Null
Set-Item -LiteralPath "$key\shell\open\command" -Value $command
$runKey = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Run'
if (Test-Path -LiteralPath $runKey) {
    $oldStartup = (Get-Item -LiteralPath $runKey).GetValue('Uplink')
    if ($oldStartup -and $oldStartup -like ('"' + $base + '\*\uplink.exe" --background')) {
        Set-ItemProperty -LiteralPath $runKey -Name Uplink -Value ('"' + (Join-Path $install 'uplink.exe') + '" --background')
    }
}
Write-Output $install
