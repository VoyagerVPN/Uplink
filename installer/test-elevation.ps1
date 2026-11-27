$ErrorActionPreference='Stop'
$identity=[Security.Principal.WindowsIdentity]::GetCurrent()
$principal=[Security.Principal.WindowsPrincipal]::new($identity)
if (!$principal.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)) {
    throw 'Run this test from an elevated session so the test cannot prompt for UAC.'
}
$launcher=Join-Path $PSScriptRoot 'run-elevated.ps1'
$shell=Join-Path $env:SystemRoot 'System32\WindowsPowerShell\v1.0\powershell.exe'
$command=Join-Path $env:SystemRoot 'System32\cmd.exe'
& $shell -NoLogo -NoProfile -NonInteractive -ExecutionPolicy Bypass -File $launcher -Executable $command -ArgumentLine '/d /c exit 37'
if ($LASTEXITCODE -ne 37) {throw "The elevated launcher returned $LASTEXITCODE instead of child exit 37"}
& $shell -NoLogo -NoProfile -NonInteractive -ExecutionPolicy Bypass -File $launcher -Executable $command -ArgumentLine '/d /c exit 0'
if ($LASTEXITCODE -ne 0) {throw "The elevated launcher returned $LASTEXITCODE instead of child exit 0"}
$missing=Join-Path $env:SystemRoot ('uplink-qa-'+[guid]::NewGuid().ToString('N')+'.exe')
& $shell -NoLogo -NoProfile -NonInteractive -ExecutionPolicy Bypass -File $launcher -Executable $missing -ArgumentLine '/S'
if ($LASTEXITCODE -ne 65538) {throw "The elevated launcher returned $LASTEXITCODE instead of Win32 launch error 2"}
Write-Output 'Elevated child exit codes and Windows launch errors passed.'
exit 0
