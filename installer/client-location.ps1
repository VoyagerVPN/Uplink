param([Parameter(Mandatory)][ValidateSet('Install','Remove')][string]$Action,
      [Parameter(Mandatory)][ValidatePattern('^(Uplink|Uplink-QA-[a-z0-9]+)$')][string]$Product,
      [string]$Destination, [ValidatePattern('^[0-9A-F]{20}$')][string]$Tag)
$ErrorActionPreference='Stop'
$env:PSModulePath=Join-Path $PSHOME 'Modules'
$installStep=1
trap {
    if ($Action -ne 'Install') {break}
    # Старшие биты обозначают этап, младшие содержат код исключения без текста и путей.
    $code=$_.Exception.GetBaseException().HResult -band 65535
    exit (($installStep -shl 16) -bor $code)
}
$key="HKCU:\Software\$Product\Installer"
$arp="HKLM:\Software\Microsoft\Windows\CurrentVersion\Uninstall\$Product"
$scheme=if ($Product -eq 'Uplink') {'uplink'} else {$Product}
$link=Join-Path ([Environment]::GetFolderPath('Programs')) "$Product.lnk"
$protocol="HKCU:\Software\Classes\$scheme"
$runKey="HKCU:\Software\Microsoft\Windows\CurrentVersion\Run"
$files='uplink.exe','uplink-window.exe','uplinkctl.exe','package.id','Inter-LICENSE.txt'
function Check-Directory([string]$Path) {
    $dir=[IO.DirectoryInfo]::new($Path)
    while ($dir) {
        if ((Test-Path -LiteralPath $dir.FullName) -and ($dir.Attributes -band [IO.FileAttributes]::ReparsePoint)) {throw 'Redirected installation directory'}
        $dir=$dir.Parent
    }
}
function Package-Marker([string]$Path) {
    Check-Directory $Path
    $tag=[IO.Path]::GetFileName($Path)
    $marker=Join-Path $Path 'package.id'
    if ($tag -cnotmatch '^[0-9A-F]{20}$' -or !(Test-Path -LiteralPath $marker -PathType Leaf)) {return $false}
    if ((Get-Item -LiteralPath $marker -Force).Attributes -band [IO.FileAttributes]::ReparsePoint) {return $false}
    return ([IO.File]::ReadAllText($marker) -ceq "$Product|$tag")
}
function Remove-Empty([string]$Path) {
    if ([IO.Directory]::Exists($Path) -and !(Get-ChildItem -LiteralPath $Path -Force)) {[IO.Directory]::Delete($Path)}
}
$state=Get-ItemProperty -LiteralPath $key -ErrorAction SilentlyContinue
$legacyPath=$arp.Replace('HKLM:','HKCU:')
$legacyEntry=Get-ItemProperty -LiteralPath $legacyPath -ErrorAction SilentlyContinue
$legacy=if ($legacyEntry.DisplayName -eq $Product -and $legacyEntry.Publisher -eq 'Uplink') {$legacyEntry.InstallLocation}
$oldExe=if ($state.InstallLocation) {Join-Path $state.InstallLocation 'uplink.exe'}
$hook=Get-Item -LiteralPath "$protocol\shell\open\command" -ErrorAction SilentlyContinue
$command=if ($hook) {$hook.GetValue('')}
$run=Get-Item -LiteralPath $runKey -ErrorAction SilentlyContinue
$autostart=if ($run) {$run.GetValue($Product)}
if ($Action -eq 'Remove') {
    if (!$state) {return}
    if ($command -eq ('"'+$oldExe+'" "%1"')) {Remove-Item -LiteralPath $protocol -Recurse}
    if ($autostart -eq ('"'+$oldExe+'" --background')) {Remove-ItemProperty -LiteralPath $runKey -Name $Product}
    if (Test-Path -LiteralPath $link) {
        $shortcut=(New-Object -ComObject WScript.Shell).CreateShortcut($link)
        if ($shortcut.TargetPath -eq $oldExe) {[IO.File]::Delete($link)}
    }
    foreach ($dir in $state.Packages) {
        if (!(Package-Marker $dir)) {continue}
        foreach ($name in $files) {[IO.File]::Delete((Join-Path $dir $name))}
        Remove-Empty $dir
        Remove-Empty ([IO.Path]::GetDirectoryName($dir))
    }
    foreach ($dir in $state.CreatedRoots) {Check-Directory $dir; Remove-Empty $dir}
    if ($legacy -and $legacy -eq $state.InstallLocation -and $legacyEntry.UplinkMigrated -eq 1) {
        Remove-Item -LiteralPath $legacyPath -Recurse
    }
    Remove-Item -LiteralPath $key
    return
}
$machine=Get-ItemProperty -LiteralPath $arp
$source=$machine.InstallLocation
if ([IO.Path]::GetFileName($source) -cne $Tag -or !(Package-Marker $source)) {throw 'Incomplete system package'}
$installStep=2
$root=[IO.Path]::GetFullPath($Destination).TrimEnd('\')
if ($root -notmatch '^[A-Za-z]:\\' -or $root.StartsWith($machine.PackageRoot+'\',[StringComparison]::OrdinalIgnoreCase)) {throw 'Invalid installation folder'}
$dir=$source
$custom=$root -ne $machine.PackageRoot
if ($custom) {
    $packages=Join-Path $root 'packages'
    $dir=Join-Path $packages ([IO.Path]::GetFileName($source))
    Check-Directory $dir
    $newRoot=!(Test-Path -LiteralPath $root)
    [IO.Directory]::CreateDirectory($packages) | Out-Null
    if (![IO.Directory]::Exists($dir)) {
        $stage=Join-Path $packages ([guid]::NewGuid().ToString('N'))
        [IO.Directory]::CreateDirectory($stage) | Out-Null
        try {
            foreach ($file in $files) {[IO.File]::Copy((Join-Path $source $file),(Join-Path $stage $file))}
            [IO.Directory]::Move($stage,$dir)
        } finally {
            if ([IO.Directory]::Exists($stage)) {
                foreach ($file in $files) {[IO.File]::Delete((Join-Path $stage $file))}
                Remove-Empty $stage
            }
        }
    } else {
        if (!(Package-Marker $dir)) {throw 'Unrecognized existing client directory'}
        foreach ($file in $files) {
            $target=Get-Item -LiteralPath (Join-Path $dir $file)
            if (($target.Attributes -band [IO.FileAttributes]::ReparsePoint) -or (Get-FileHash -LiteralPath $target.FullName).Hash -ne (Get-FileHash -LiteralPath (Join-Path $source $file)).Hash) {throw 'Existing client differs'}
        }
    }
}
$exe=Join-Path $dir 'uplink.exe'
if (!$state -and !$custom -and !$hook -and !$legacy) {return}
$installStep=3
if (!(Test-Path -LiteralPath $key)) {New-Item -Path $key -Force | Out-Null}
if ($custom) {
    $history=@(@($state.Packages) + $dir | Where-Object {$_} | Select-Object -Unique)
    New-ItemProperty -LiteralPath $key -Name Packages -PropertyType MultiString -Value $history -Force | Out-Null
    if ($newRoot) {
        $created=@(@($state.CreatedRoots) + $root | Where-Object {$_} | Select-Object -Unique)
        New-ItemProperty -LiteralPath $key -Name CreatedRoots -PropertyType MultiString -Value $created -Force | Out-Null
    }
}
foreach ($item in @{PackageRoot=$root; InstallLocation=$dir}.GetEnumerator()) {
    New-ItemProperty -LiteralPath $key -Name $item.Key -Value $item.Value -Force | Out-Null
}
$owned=@($oldExe,(Join-Path $source 'uplink.exe'))
if ($legacy) {$owned+=Join-Path $legacy 'uplink.exe'}
if (!$command -or $command -in @($owned | ForEach-Object {'"'+$_+'" "%1"'})) {
    New-Item -Path "$protocol\shell\open\command" -Force | Out-Null
    Set-Item -LiteralPath $protocol -Value 'URL:Uplink Invitation'
    New-ItemProperty -LiteralPath $protocol -Name 'URL Protocol' -Value '' -Force | Out-Null
    Set-Item -LiteralPath "$protocol\shell\open\command" -Value ('"'+$exe+'" "%1"')
}
if ($autostart -and $autostart -in @($owned | ForEach-Object {'"'+$_+'" --background'})) {
    Set-ItemProperty -LiteralPath $runKey -Name $Product -Value ('"'+$exe+'" --background')
}
$installStep=4
$shortcut=(New-Object -ComObject WScript.Shell).CreateShortcut($link)
$shortcut.TargetPath=$exe
$shortcut.WorkingDirectory=$dir
$shortcut.Save()
if ($legacy) {
    $installStep=3
    # Апдейтер 0.2.0 сверяет хеш и запускает новый клиент по старой записи HKCU.
    New-ItemProperty -LiteralPath $legacyPath -Name SystemComponent -PropertyType DWord -Value 1 -Force | Out-Null
    New-ItemProperty -LiteralPath $legacyPath -Name UplinkMigrated -PropertyType DWord -Value 1 -Force | Out-Null
    Set-ItemProperty -LiteralPath $legacyPath -Name InstallLocation -Value $dir
}
