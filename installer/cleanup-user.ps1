param(
    [Parameter(Mandatory)][ValidatePattern('^(Uplink|Uplink-QA-[a-z0-9]+)$')][string]$Product,
    [switch]$Purge
)
$ErrorActionPreference='Stop'
$env:PSModulePath=Join-Path $PSHOME 'Modules'
$local=[Environment]::GetFolderPath('LocalApplicationData')
$roaming=[Environment]::GetFolderPath('ApplicationData')
$profile=Join-Path $roaming $Product
$localData=Join-Path $local $Product
function Check-Path($path) {
    $item=Get-Item -LiteralPath $path -Force -ErrorAction SilentlyContinue
    if ($item -and $item.Attributes -band [IO.FileAttributes]::ReparsePoint) {throw 'Unexpected redirected Uplink directory'}
}
function Remove-Empty($path) {
    if ([IO.Directory]::Exists($path) -and !(Get-ChildItem -LiteralPath $path -Force)) {[IO.Directory]::Delete($path)}
}
function Clear-Directory($path) {
    Check-Path $path
    foreach ($item in Get-ChildItem -LiteralPath $path -Force) {
        if ($item.Attributes -band [IO.FileAttributes]::ReparsePoint) {
            if ($item.PSIsContainer) {[IO.Directory]::Delete($item.FullName)} else {[IO.File]::Delete($item.FullName)}
        } elseif ($item.PSIsContainer) {Clear-Directory $item.FullName}
        else {[IO.File]::Delete($item.FullName)}
    }
    [IO.Directory]::Delete($path)
}
foreach ($p in $profile,$localData) {Check-Path $p}
if ($Purge) {
    $key="Software\$Product"
    $settings=[Microsoft.Win32.Registry]::CurrentUser.OpenSubKey($key,$true)
    if ($settings) {
        try { $settings.DeleteSubKeyTree('Networks',$false) } finally { $settings.Dispose() }
    }
}
$requests=Join-Path $localData 'requests'
Check-Path $requests
if ([IO.Directory]::Exists($requests)) {
    foreach ($request in Get-ChildItem -File -Force -LiteralPath $requests) {
        if ($request.Name -cmatch '^[0-9a-f]{32}$') {[IO.File]::Delete($request.FullName)}
    }
    Remove-Empty $requests
}
Remove-Empty $localData
if ([IO.Directory]::Exists($profile)) {
    [IO.File]::Delete((Join-Path $profile 'updates.json'))
    $updates=Join-Path $profile 'update-cache'
    Check-Path $updates
    if ([IO.Directory]::Exists($updates)) {
        foreach ($file in Get-ChildItem -LiteralPath $updates -File -Filter 'uplink-update-*.exe') {[IO.File]::Delete($file.FullName)}
        Remove-Empty $updates
    }
    $rootsKey=Get-Item -LiteralPath "HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\$Product\Roots" -ErrorAction SilentlyContinue
    $sharedFolder=$rootsKey -and @($rootsKey.GetValueNames() | Where-Object { $_ -eq $profile -or $_.StartsWith($profile+'\',[StringComparison]::OrdinalIgnoreCase) }).Count -gt 0
    if ($Purge -and !$sharedFolder) {Clear-Directory $profile}
    elseif ($Purge) {
        foreach ($f in Get-ChildItem -File -Force -LiteralPath $profile) {
            if ($f.Name -match '^(prefs\.json($|\.)|prefs\.lock$|transport\.lock$|\.tmp)') {[IO.File]::Delete($f.FullName)}
        }
        Remove-Empty $profile
    }
    else {
        foreach ($f in 'transport.lock','prefs.lock') {[IO.File]::Delete((Join-Path $profile $f))}
        Remove-Empty $profile
    }
}
