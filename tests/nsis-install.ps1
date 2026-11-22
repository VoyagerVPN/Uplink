param([Parameter(Mandatory)][string]$Setup, [Parameter(Mandatory)][ValidatePattern('^Uplink-QA-[a-z0-9]+$')][string]$Product)
$ErrorActionPreference='Stop'
$root=Join-Path $env:LOCALAPPDATA "Programs/$Product"
$key="HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\$Product"
$protocol="HKCU:\Software\Classes\$Product"
if ((Test-Path $root) -or (Test-Path $key) -or (Test-Path $protocol)) {throw 'Test namespace already exists'}
function Run([string]$exe,[string]$arguments,[int]$expected=0) {
    $p=Start-Process -FilePath $exe -ArgumentList $arguments -WindowStyle Hidden -Wait -PassThru
    if ($p.ExitCode -ne $expected) {throw "Expected $expected, got $($p.ExitCode)"}
}
# Предыдущая установка использовала регистрацию install-user.ps1.
$runKey='HKCU:\Software\Microsoft\Windows\CurrentVersion\Run'
if ((Get-Item -LiteralPath $runKey).GetValue($Product)) {throw 'Test autostart already exists'}
New-Item -Path "$protocol\shell\open\command" -Force | Out-Null
Set-Item -LiteralPath $protocol -Value 'Foreign protocol'
Run $Setup '/S' 21
if ((Test-Path $key) -or (Test-Path $root)) {throw 'Conflict modified the installation'}
$bin=Join-Path $PSScriptRoot '../.cache/target/perf'
$names='uplink.exe','uplink-window.exe','uplinkctl.exe'
$tag=($names | ForEach-Object {(Get-FileHash -LiteralPath (Join-Path $bin $_)).Hash.Substring(0,8)}) -join '-'
$old=New-Item -ItemType Directory -Path (Join-Path $root $tag) -Force
foreach ($name in $names) {Copy-Item -LiteralPath (Join-Path $bin $name) -Destination $old.FullName}
$legacy=Join-Path $old.FullName 'uplink.exe'
Set-Item -LiteralPath $protocol -Value 'URL:Uplink Invitation'
Set-Item -LiteralPath "$protocol\shell\open\command" -Value ('"'+$legacy+'" "%1"')
New-ItemProperty -LiteralPath $runKey -Name $Product -Value ('"'+$legacy+'" --background') -Force | Out-Null
Run $Setup '/S'
$dir=Get-ItemPropertyValue $key InstallLocation
if (!(Test-Path -LiteralPath (Join-Path $dir 'uplinkctl.exe'))) {throw 'CLI missing'}
if ((Get-Item "$protocol\shell\open\command").GetValue('') -ne ('"'+(Join-Path $dir 'uplink.exe')+'" "%1"')) {throw 'Protocol mismatch'}
$shortcut=Join-Path ([Environment]::GetFolderPath('Programs')) "$Product.lnk"
if (!(Test-Path -LiteralPath $shortcut)) {throw 'Start shortcut missing'}
if ((Get-Item -LiteralPath $runKey).GetValue($Product) -ne ('"'+(Join-Path $dir 'uplink.exe')+'" --background')) {throw 'Autostart was not migrated'}
$sentinel=Join-Path $dir 'my-file.txt'
Set-Content -LiteralPath $sentinel 'keep'
$first=$dir
$custom=Join-Path (Resolve-Path (Join-Path $PSScriptRoot '../.cache')).Path ("Папка с пробелами "+$Product)
if (Test-Path $custom) {throw 'Test destination already exists'}
Run $Setup ('/S /D='+$custom)
$dir=Get-ItemPropertyValue $key InstallLocation
if (!(Test-Path -LiteralPath (Join-Path $dir 'uplink.exe')) -or (Split-Path (Split-Path $dir)) -ne $custom) {throw 'Selected directory was ignored'}
Run $Setup '/S'
if ((Get-ItemPropertyValue $key InstallLocation) -ne $dir) {throw 'Update lost the selected directory'}
$copy=Join-Path (Split-Path -LiteralPath (Resolve-Path $Setup).Path) "$Product-uninstall.exe"
Copy-Item -LiteralPath (Join-Path $dir 'Uninstall.exe') -Destination $copy
$busy=[IO.File]::Open((Join-Path $dir 'uplink.exe'),'Open','Read','Read')
try {Run $copy ('/S _?='+$dir) 22} finally {$busy.Dispose()}
if (!(Test-Path $key)) {throw 'Busy uninstall removed registration'}
Run $copy ('/S _?='+$dir)
if ((Get-Item -LiteralPath $runKey).GetValue($Product)) {throw 'Autostart entry remains'}
if ((Test-Path $key) -or (Test-Path $protocol) -or (Test-Path $shortcut) -or (Test-Path (Join-Path $dir 'uplink.exe'))) {throw 'Uninstall incomplete'}
if (Test-Path -LiteralPath $old.FullName) {throw 'Legacy hash-named release survived uninstall'}
if (Test-Path (Join-Path $first 'uplink.exe')) {throw 'Previous location still has an installed package'}
if ((Get-Content -LiteralPath $sentinel) -ne 'keep') {throw 'User file removed'}
Remove-Item -LiteralPath $sentinel,$copy
foreach ($p in $dir,(Split-Path $dir),$custom,$first,(Split-Path $first),$root) {if ((Test-Path -LiteralPath $p) -and !(Get-ChildItem -Force -LiteralPath $p)) {[IO.Directory]::Delete($p)}}
'Install, repeat install, URL association, Start shortcut and uninstall OK'
