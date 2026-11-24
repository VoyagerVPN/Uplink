param([Parameter(Mandatory)][string]$Setup, [ValidatePattern('^Uplink-QA-[a-z0-9]+$')][string]$Product='Uplink-QA-serviceverify')
$ErrorActionPreference='Stop'
$exe=(Resolve-Path -LiteralPath $Setup).Path
if ([IO.Path]::GetFileName($exe) -ne "$Product-Setup.exe") {throw 'Use a matching QA setup'}
$arp="HKLM:\Software\Microsoft\Windows\CurrentVersion\Uninstall\$Product"
$key="HKCU:\Software\$Product\Installer"
if ((Test-Path $arp) -or (Test-Path $key)) {throw 'QA namespace already exists'}
function Invoke-Setup([string]$file,[string]$argsLine) {
    $proc=Start-Process -FilePath $file -ArgumentList $argsLine -WindowStyle Hidden -Wait -PassThru
    if ($proc.ExitCode) {throw "Setup returned $($proc.ExitCode)"}
}
$dir=Join-Path ((Resolve-Path "$PSScriptRoot/../.cache").Path) ('Установка QA '+[guid]::NewGuid())
try {
    Invoke-Setup $exe "/S /D=$dir"
    $client=(Get-ItemPropertyValue $key InstallLocation) + '\uplink.exe'
    if (!(Test-Path -LiteralPath $client) -or (Split-Path (Split-Path (Split-Path $client))) -ne $dir) {throw 'Custom client missing'}
    Invoke-Setup $exe '/S'
    if ((Get-ItemPropertyValue $key PackageRoot) -ne $dir -or !(Test-Path -LiteralPath $client)) {throw 'Update lost custom location'}
    if ((Get-ItemPropertyValue "HKLM:\SYSTEM\CurrentControlSet\Services\$($Product)Network" ImagePath) -notlike ('"'+$env:ProgramFiles+'\*')) {throw 'Service outside protected directory'}
    [IO.File]::WriteAllText((Join-Path $dir 'user-file.txt'),'keep')
} finally {
    if (Test-Path $arp) {
        $machine=Get-ItemPropertyValue $arp InstallLocation
        $un=Join-Path ((Resolve-Path "$PSScriptRoot/../.cache").Path) "$Product-location-un.exe"
        Copy-Item -LiteralPath (Join-Path $machine 'Uninstall.exe') -Destination $un -Force
        try {Invoke-Setup $un "/S _?=$machine"} finally {Remove-Item -LiteralPath $un}
    }
}
if ((Test-Path -LiteralPath $client) -or (Test-Path $key) -or ((Get-Content -LiteralPath (Join-Path $dir 'user-file.txt')) -ne 'keep')) {throw 'Uninstall cleanup mismatch'}
Remove-Item -LiteralPath (Join-Path $dir 'user-file.txt')
[IO.Directory]::Delete($dir)
'Custom client location, remembered silent reinstall, protected service and cleanup passed.'
