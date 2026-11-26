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
function Clear-SshKeys {
    $userProfile=[Environment]::GetFolderPath('UserProfile')
    if (![IO.Path]::IsPathRooted($userProfile)) {throw 'Windows user profile is unavailable'}
    $ssh=Join-Path $userProfile '.ssh'
    $keys=Join-Path $ssh $Product.ToLowerInvariant()
    foreach ($path in $ssh,$keys) {Check-Path $path}
    if (![IO.Directory]::Exists($keys)) {return}
    foreach ($item in Get-ChildItem -LiteralPath $keys -Force) {
        if (!$item.PSIsContainer -and $item.Name -cmatch '^[0-9a-f]{64}\.key$') {
            Check-Path $item.FullName
            [IO.File]::Delete($item.FullName)
        } elseif ($item.PSIsContainer -and $item.Name -cmatch '^\.key-[A-Za-z0-9]{16}$') {
            Check-Path $item.FullName
            foreach ($name in 'identity','identity.pub') {
                $file=Join-Path $item.FullName $name
                Check-Path $file
                [IO.File]::Delete($file)
            }
            Remove-Empty $item.FullName
        }
    }
    Remove-Empty $keys
}
foreach ($p in $profile,$localData) {Check-Path $p}
if ($Purge) {
    Clear-SshKeys
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
    foreach ($log in 'last-join.log','transport.log','transport.previous.log','transport.log.lock','peer-ping.log','peer-ping.previous.log','peer-ping.log.lock','update.log','update.previous.log','update.log.lock') {
        [IO.File]::Delete((Join-Path $profile $log))
    }
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
