function Get-ZeroTierProducts {
    foreach ($view in 'Registry64','Registry32') {
        $hive=[Microsoft.Win32.RegistryKey]::OpenBaseKey('LocalMachine',$view)
        $key=$hive.OpenSubKey('Software\Microsoft\Windows\CurrentVersion\Uninstall')
        foreach ($id in $key.GetSubKeyNames()) {
            if ($id -notmatch '^\{[0-9a-f-]{36}\}$') {continue}
            $entry=$key.OpenSubKey($id)
            if ($entry.GetValue('DisplayName') -eq 'ZeroTier One' -and $entry.GetValue('Publisher') -eq 'ZeroTier, Inc.' -and $entry.GetValue('WindowsInstaller') -eq 1) {$id}
            $entry.Dispose()
        }
        $key.Dispose()
        $hive.Dispose()
    }
}

function Remove-NetworkComponents([string]$base,[string]$ownerSid,[string]$action) {
$homeDir=Join-Path $base 'One'
$proof=Join-Path $homeDir 'uplink-proof'
$receipt=Join-Path $homeDir '.uplink-owner'
function Assert-Ordinary($p) {
    if (Test-Path -LiteralPath $p) {
        if ((Get-Item -LiteralPath $p -Force).Attributes -band [IO.FileAttributes]::ReparsePoint) {throw 'Unsafe transport path'}
    }
}
function Assert-MachineDirectory($p) {
    Assert-Ordinary $p
    if (!(Test-Path -LiteralPath $p)) {return}
    $trusted=@('S-1-5-18','S-1-5-32-544')
    $acl=Get-Acl -LiteralPath $p
    if ($acl.GetOwner([Security.Principal.SecurityIdentifier]).Value -notin $trusted) {throw 'Unknown transport owner'}
    foreach ($ace in $acl.GetAccessRules($true,$true,[Security.Principal.SecurityIdentifier])) {
        if ($ace.AccessControlType -eq 'Allow' -and $ace.IdentityReference.Value -notin $trusted -and ([int]$ace.FileSystemRights -band 0xD0156)) {throw 'Unsafe transport permissions'}
    }
}
# Обход без перехода через ссылки. Удаляем только внутри явно выбранного каталога.
function Clear-Tree($p) {
    Assert-MachineDirectory $p
    foreach ($f in Get-ChildItem -Force -LiteralPath $p) {
        Assert-Ordinary $f.FullName
        if ($f.PSIsContainer) {Clear-Tree $f.FullName}
        else {[IO.File]::Delete($f.FullName)}
    }
    [IO.Directory]::Delete($p)
}
foreach ($p in $base,$homeDir) {Assert-MachineDirectory $p}
if ($action -eq 'remove') {
    $pending=Join-Path $homeDir '.uplink-removing'
    Assert-Ordinary $pending
    $resuming=(Test-Path -LiteralPath $pending) -and [IO.File]::ReadAllText($pending) -eq $ownerSid
    $products=@(Get-ZeroTierProducts)
    if (!$products -and !$resuming -and ((Get-Service ZeroTierOneService -ErrorAction SilentlyContinue) -or ((Test-Path -LiteralPath $homeDir) -and !(Test-Path -LiteralPath $receipt)))) {throw 'Unknown ZeroTier package'}
    if ($products -and (Test-Path -LiteralPath $homeDir)) {[IO.File]::WriteAllText($pending,$ownerSid,[Text.Encoding]::ASCII)}
    foreach ($id in ($products | Select-Object -Unique)) {
        $p=Start-Process -FilePath (Join-Path ([Environment]::SystemDirectory) 'msiexec.exe') -ArgumentList "/x $id /qn /norestart" -WindowStyle Hidden -Wait -PassThru
        if ($p.ExitCode -eq 3010) {throw 'RESTART_REQUIRED'}
        if ($p.ExitCode -notin 0,1605) {throw "MSI $($p.ExitCode)"}
    }
    if ($resuming -and !$products -and (Get-Service ZeroTierOneService -ErrorAction SilentlyContinue)) {throw 'RESTART_REQUIRED'}
    if (Test-Path -LiteralPath $homeDir) {Clear-Tree $homeDir}
    if ((Test-Path -LiteralPath $base) -and !(Get-ChildItem -LiteralPath $base -Force)) {[IO.Directory]::Delete($base)}
    return
}
if (Test-Path -LiteralPath $proof) {
    Assert-MachineDirectory $proof
    $acl=Get-Acl -LiteralPath $proof
    if ($acl.GetAccessRules($true,$true,[Security.Principal.SecurityIdentifier]).IdentityReference.Value -contains $ownerSid) {
        Clear-Tree $proof
        $token=Join-Path $homeDir 'authtoken.secret'
        Assert-Ordinary $token
        if (Test-Path -LiteralPath $token) {
            $acl=Get-Acl -LiteralPath $token
            foreach ($ace in $acl.GetAccessRules($true,$false,[Security.Principal.SecurityIdentifier])) {
                if ($ace.IdentityReference.Value -eq $ownerSid -and $ace.AccessControlType -eq 'Allow' -and $ace.FileSystemRights -eq ([Security.AccessControl.FileSystemRights]'Read, Synchronize')) {$acl.RemoveAccessRuleSpecific($ace)}
            }
            Set-Acl -LiteralPath $token -AclObject $acl
        }
    }
}
Assert-Ordinary $receipt
if ((Test-Path -LiteralPath $receipt) -and [IO.File]::ReadAllText($receipt) -eq $ownerSid) {[IO.File]::Delete($receipt)}
}
