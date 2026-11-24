param([Parameter(Mandatory)][string]$Setup, [Parameter(Mandatory)][string]$PreviousClient,
      [ValidatePattern('^Uplink-QA-[a-z0-9]+$')][string]$Product='Uplink-QA-upgrade021')
$ErrorActionPreference='Stop'
$exe=(Resolve-Path -LiteralPath $Setup).Path
$old=Get-Item -LiteralPath $PreviousClient
if ([IO.Path]::GetFileName($exe) -ne "$Product-Setup.exe" -or $old.VersionInfo.FileVersion -ne '0.2.0') {throw 'Expected a QA installer and a 0.2.0 client'}
$subkey="Software\Microsoft\Windows\CurrentVersion\Uninstall\$Product"
$legacy="HKCU:\$subkey"
$machine="HKLM:\$subkey"
$config="HKCU:\Software\$Product\Installer"
foreach ($path in $legacy,$machine,$config,"HKCU:\Software\Classes\$Product","HKLM:\Software\Classes\$Product") {
    if (Test-Path -LiteralPath $path) {throw 'QA product is already installed'}
}
$base=Join-Path ((Resolve-Path "$PSScriptRoot/../.cache").Path) ('upgrade-'+[guid]::NewGuid())
$previous=Join-Path $base 'packages\00000000000000000200'
function Update-Client {
    (Start-Process $exe -ArgumentList '/S /UPLINKUPDATE' -WindowStyle Hidden -Wait -PassThru).ExitCode
}
try {
    [IO.Directory]::CreateDirectory($previous) | Out-Null
    [IO.File]::Copy($old.FullName,(Join-Path $previous 'uplink.exe'))
    [IO.File]::WriteAllText((Join-Path $previous 'package.id'),"$Product|00000000000000000200")
    New-Item -Path $legacy -Force | Out-Null
    foreach ($item in @{DisplayName=$Product;InstallLocation=$previous;PackageRoot=$base;Publisher='Uplink'}.GetEnumerator()) {
        New-ItemProperty -LiteralPath $legacy -Name $item.Key -Value $item.Value | Out-Null
    }
    $rc=Update-Client
    if ($rc -ne 0) {throw "Update from the per-user installation failed ($rc)"}
    $dir=Get-ItemPropertyValue $legacy InstallLocation
    $client=Get-Item -LiteralPath (Join-Path $dir 'uplink.exe')
    if ($client.VersionInfo.FileVersion -ne (Get-Item $exe).VersionInfo.FileVersion -or
        (Get-FileHash $client.FullName).Hash -ne (Get-FileHash "$PSScriptRoot/../.cache/target/perf/uplink.exe").Hash -or
        (Get-ItemPropertyValue $config PackageRoot) -ne $base) {throw 'The old updater did not find the new verified client'}
    if ((Update-Client) -ne 28) {throw 'Update accepted the same version twice'}
    'Per-user 0.2.0 upgrade, original client lookup, custom directory and version guard passed.'
} finally {
    if (Test-Path -LiteralPath $machine) {
        $installed=Get-ItemPropertyValue $machine InstallLocation
        $un=Join-Path $base 'qa-uninstall.exe'
        [IO.File]::Copy((Join-Path $installed 'Uninstall.exe'),$un)
        $result=Start-Process $un -ArgumentList ('/S _?='+$installed) -WindowStyle Hidden -Wait -PassThru
        if ($result.ExitCode) {throw "QA uninstall failed: $($result.ExitCode)"}
        [IO.File]::Delete($un)
    }
    if (Test-Path -LiteralPath $legacy) {Remove-Item -LiteralPath $legacy}
    foreach ($file in 'uplink.exe','package.id') {[IO.File]::Delete((Join-Path $previous $file))}
    foreach ($dir in $previous,(Split-Path $previous),$base) {
        if ([IO.Directory]::Exists($dir)) {[IO.Directory]::Delete($dir)}
    }
}
