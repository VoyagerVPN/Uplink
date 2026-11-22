param(
    [Parameter(Mandatory)][ValidatePattern('^(Uplink|Uplink-QA-[a-z0-9]+)$')][string]$Product,
    [Parameter(Mandatory)][string]$Helper,
    [switch]$Purge,[switch]$RemoveZeroTier
)
$ErrorActionPreference='Stop'
$env:PSModulePath=Join-Path $PSHOME 'Modules'
$local=[Environment]::GetFolderPath('LocalApplicationData')
$roaming=[Environment]::GetFolderPath('ApplicationData')
$base=Join-Path $local "Programs\$Product"
$profile=Join-Path $roaming $Product
$localData=Join-Path $local $Product
$files='uplink.exe','uplink-window.exe','uplinkctl.exe'
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
foreach ($p in (Join-Path $local 'Programs'),$base,$profile,$localData) {Check-Path $p}
if ($Product -ceq 'Uplink') {
    $zt=Join-Path ([Environment]::GetFolderPath('CommonApplicationData')) 'ZeroTier\One'
    $proof=Join-Path $zt 'uplink-proof'
    $receipt=Join-Path $zt '.uplink-owner'
    $config=Join-Path $profile 'prefs.json'
    if (!$RemoveZeroTier -and (Test-Path -LiteralPath $config) -and (Test-Path -LiteralPath (Join-Path $zt 'zerotier-one_x64.exe'))) {
        $prefs=Get-Content -LiteralPath $config -Raw -Encoding UTF8 | ConvertFrom-Json
        $ids=@(foreach ($map in $prefs.live,$prefs.pending) {
            if ($null -eq $map) {continue}
            foreach ($row in $map.PSObject.Properties.Value) {if ($row.transport) {$row.transport} else {$row.id}}
        }; if ($prefs.shared_devices) {$prefs.shared_devices.PSObject.Properties.Name})
        foreach ($id in ($ids | Where-Object {$_ -cmatch '^[0-9a-f]{16}$'} | Select-Object -Unique)) {
            # Уже отсоединённая сеть также считается очищенной.
            if (Test-Path -LiteralPath (Join-Path $zt "networks.d\$id.conf")) {
                & (Join-Path $zt 'zerotier-one_x64.exe') -q leave $id | Out-Null
                if ($LASTEXITCODE) {throw 'Could not disconnect an Uplink network'}
            }
        }
    }
    if ($RemoveZeroTier -or (Test-Path -LiteralPath $proof) -or (Test-Path -LiteralPath $receipt)) {
        $mode=if ($RemoveZeroTier) {'remove'} else {'clean'}
        $sid=[Security.Principal.WindowsIdentity]::GetCurrent().User.Value
        $run=Start-Process -FilePath $Helper -Verb RunAs -ArgumentList "$mode $sid" -Wait -PassThru -WindowStyle Hidden
        if ($run.ExitCode -eq 29) {exit 29}
        if ($run.ExitCode) {throw 'Transport cleanup failed'}
    }
}
# Старый install-user.ps1 называл каталог по трём SHA-256. Проверяем каждый файл перед удалением.
if (Test-Path -LiteralPath $base) {
    foreach ($dir in Get-ChildItem -LiteralPath $base -Directory) {
        if ($dir.Name -notmatch '^[0-9a-f]{8}-[0-9a-f]{8}-[0-9a-f]{8}$') {continue}
        Check-Path $dir.FullName
        $parts=$dir.Name.Split('-')
        $matched=$false
        for ($i=0;$i -lt $files.Count;$i++) {
            $path=Join-Path $dir.FullName $files[$i]
            if (!(Test-Path -LiteralPath $path -PathType Leaf)) {continue}
            Check-Path $path
            if (!(Get-FileHash -LiteralPath $path).Hash.StartsWith($parts[$i],[StringComparison]::OrdinalIgnoreCase)) {continue}
            $matched=$true
            [IO.File]::Delete($path)
        }
        if ($matched) {[IO.File]::Delete((Join-Path $dir.FullName 'Inter-LICENSE.txt'))}
        Remove-Empty $dir.FullName
    }
    Remove-Empty $base
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
