#Requires -RunAsAdministrator
[CmdletBinding(DefaultParameterSetName='Link')]
param(
    [Parameter(Mandatory,ParameterSetName='Link')][string]$Invitation,
    [Parameter(Mandatory,ParameterSetName='Input')][switch]$ReadInvitation,
    [switch]$Headless
)
if ($ReadInvitation) { $Invitation = [Console]::In.ReadLine() }
$ErrorActionPreference = 'Stop'

if ($Invitation -cnotmatch '^uplink://(?<server>[a-zA-Z0-9.-]+)/invite/(?<token>[a-f0-9]{64})$') {
    throw 'Ожидается ссылка-приглашение Uplink.'
}
$address = 'https://' + $Matches.server + '/v1/bootstrap'
$body = @{token=$Matches.token; version=2} | ConvertTo-Json -Compress
$info = Invoke-RestMethod -Method Post -Uri $address -ContentType application/json -Body $body -MaximumRedirection 0
$planet = [Convert]::FromBase64String($info.planet)
if ($planet.Length -lt 100 -or $planet.Length -gt 65536) {throw 'Некорректный файл корневых узлов.'}
$base = Join-Path $env:ProgramData 'ZeroTier'
$dir = Join-Path $base 'One'
$planetPath = Join-Path $dir 'planet'
$marker = Join-Path $dir '.uplink-setup'
$who = [Security.Principal.WindowsIdentity]::GetCurrent().User.Value
$foreign = 'Обнаружена другая установка ZeroTier. Проверьте её корневую конфигурацию.'

function Assert-RegularPath([string]$path) {
    $item = Get-Item -LiteralPath $path -Force -ErrorAction SilentlyContinue
    if ($item -and ($item.Attributes -band [IO.FileAttributes]::ReparsePoint)) { throw $foreign }
}

function Assert-PrivateDirectory([string]$path) {
    Assert-RegularPath $path
    $acl = Get-Acl -LiteralPath $path
    $trusted = @('S-1-5-18','S-1-5-32-544')
    if ($acl.GetOwner([Security.Principal.SecurityIdentifier]).Value -notin $trusted) { throw $foreign }
    # Файлы конфигурации будут открываться с повышенными правами.
    $writes = [Security.AccessControl.FileSystemRights]'Write,Delete,DeleteSubdirectoriesAndFiles,ChangePermissions,TakeOwnership'
    foreach ($rule in $acl.GetAccessRules($true,$true,[Security.Principal.SecurityIdentifier])) {
        if ($rule.AccessControlType -eq 'Allow' -and ($rule.FileSystemRights -band $writes) -and
            $rule.IdentityReference.Value -notin $trusted) { throw $foreign }
    }
}

function Protect-NewDirectory([string]$path) {
    # Применяем ACL при создании, до появления каталога в общей файловой системе.
    $rules = [Security.AccessControl.DirectorySecurity]::new()
    foreach ($sid in 'S-1-5-18','S-1-5-32-544','S-1-5-32-545') {
        $principal = [Security.Principal.SecurityIdentifier]::new($sid)
        $rights = if ($sid -eq 'S-1-5-32-545') { 'ReadAndExecute' } else { 'FullControl' }
        $inherit = if ($sid -eq 'S-1-5-32-545') { 'ContainerInherit' } else { 'ContainerInherit,ObjectInherit' }
        $rule = [Security.AccessControl.FileSystemAccessRule]::new($principal,$rights,$inherit,'None','Allow')
        $rules.AddAccessRule($rule)
    }
    $rules.SetAccessRuleProtection($true,$false)
    $rules.SetOwner([Security.Principal.SecurityIdentifier]::new('S-1-5-32-544'))
    if ($PSVersionTable.PSEdition -eq 'Core') {
        [IO.FileSystemAclExtensions]::CreateDirectory($rules,$path) | Out-Null
    } else {
        [IO.Directory]::CreateDirectory($path,$rules) | Out-Null
    }
    Assert-PrivateDirectory $path
}

function Set-SecretAccess([string]$path, [bool]$clientAccess) {
    Assert-RegularPath $path
    $rules = [Security.AccessControl.FileSecurity]::new()
    $rules.SetAccessRuleProtection($true,$false)
    foreach ($sid in 'S-1-5-18','S-1-5-32-544') {
        $principal = [Security.Principal.SecurityIdentifier]::new($sid)
        $rules.AddAccessRule([Security.AccessControl.FileSystemAccessRule]::new($principal,'FullControl','Allow'))
    }
    if ($clientAccess) {
        $rules.AddAccessRule([Security.AccessControl.FileSystemAccessRule]::new(
            [Security.Principal.SecurityIdentifier]::new($who),'Read','Allow'))
    }
    Set-Acl -LiteralPath $path -AclObject $rules
}

if (Test-Path -LiteralPath $base) { Assert-PrivateDirectory $base }
$service = Get-Service ZeroTierOneService -ErrorAction SilentlyContinue
$needsMsi = !$service
$exists = Test-Path -LiteralPath $dir
if ($exists) {
    Assert-PrivateDirectory $dir
    Assert-RegularPath $planetPath
    Assert-RegularPath $marker
    if (!(Test-Path -LiteralPath $planetPath -PathType Leaf) -or
        [Convert]::ToBase64String([IO.File]::ReadAllBytes($planetPath)) -cne $info.planet) { throw $foreign }
    if (Test-Path -LiteralPath $marker) {
        if (!(Test-Path -LiteralPath $marker -PathType Leaf) -or [IO.File]::ReadAllText($marker) -cne $info.planet) { throw $foreign }
        $needsMsi = $true
    } elseif (!$service) { throw $foreign }
} elseif ($service) { throw $foreign }

if ($needsMsi) {
    $cache = Join-Path $PSScriptRoot '.cache/zerotier-129'
    New-Item -ItemType Directory -Force -Path $cache | Out-Null
    $msi = Join-Path $cache 'ZeroTierOne.msi'
    if (!(Test-Path -LiteralPath $msi)) {
        Invoke-WebRequest -Uri 'https://download.zerotier.com/RELEASES/1.16.2/dist/ZeroTierOne.msi' -OutFile $msi
    }
    if (!(Test-Path -LiteralPath $base)) { Protect-NewDirectory $base }
    $packageDir = Join-Path $base ('uplink-msi-' + [guid]::NewGuid().ToString('N'))
    Protect-NewDirectory $packageDir
    $package = Join-Path $packageDir 'ZeroTierOne.msi'
    try {
        # Проверяем и запускаем одну защищённую копию, чтобы кэш нельзя было подменить между шагами.
        [IO.File]::Copy($msi,$package)
        $signature = Get-AuthenticodeSignature -FilePath $package
        if ($signature.Status -ne 'Valid' -or $signature.SignerCertificate.Subject -notmatch 'CN="ZEROTIER, INC\."') {
            throw 'Проверка подписи MSI ZeroTier не удалась.'
        }
        if (!$exists) {
            $staging = Join-Path $base ('uplink-setup-' + [guid]::NewGuid().ToString('N'))
            Protect-NewDirectory $staging
            [IO.File]::WriteAllBytes((Join-Path $staging 'planet'),$planet)
            [IO.File]::WriteAllText((Join-Path $staging 'local.conf'),'{"settings":{"allowTcpFallbackRelay":false,"portMappingEnabled":false}}')
            [IO.File]::WriteAllText((Join-Path $staging '.uplink-setup'),$info.planet)
            [IO.Directory]::Move($staging,$dir)
        }
        # MSI меняет наследуемые ACL каталога. ZeroTier заполняет пустой identity.secret при первом запуске.
        foreach ($name in 'identity.secret','authtoken.secret','metricstoken.secret') {
            $secret = Join-Path $dir $name
            Assert-RegularPath $secret
            if (!(Test-Path -LiteralPath $secret)) {
                [IO.File]::Open($secret,'CreateNew','Write','None').Dispose()
            }
            Set-SecretAccess $secret $false
            if ($name -ne 'identity.secret' -and (Get-Item -LiteralPath $secret).Length -eq 0) {
                $bytes = [byte[]]::new(32)
                $rng = [Security.Cryptography.RandomNumberGenerator]::Create()
                try { $rng.GetBytes($bytes) } finally { $rng.Dispose() }
                [IO.File]::WriteAllText($secret, [BitConverter]::ToString($bytes).Replace('-',''))
            }
        }
        $msiexec = Join-Path $env:SystemRoot 'System32/msiexec.exe'
        $arguments = @('/i',('"{0}"' -f $package),'/qn','/norestart')
        if ($Headless) { $arguments += 'ZTHEADLESS=Yes' }
        $process = Start-Process $msiexec -ArgumentList $arguments -WindowStyle Hidden -Wait -PassThru
        if ($process.ExitCode -notin 0,3010) { throw "Код установщика ZeroTier $($process.ExitCode)." }
    } finally {
        if ([IO.File]::Exists($package)) { [IO.File]::Delete($package) }
        [IO.Directory]::Delete($packageDir)
    }
}
$service = Get-Service ZeroTierOneService -ErrorAction Stop
if (!$service) { throw 'Служба ZeroTier не установлена.' }
if ($service.Status -eq 'Stopped') { Start-Service ZeroTierOneService }
$deadline = [DateTime]::UtcNow.AddSeconds(15)
do {
    $ready = (Get-Service ZeroTierOneService).Status -eq 'Running' -and
        (Get-Item -LiteralPath (Join-Path $dir 'authtoken.secret') -ErrorAction SilentlyContinue).Length -gt 0 -and
        (Get-Item -LiteralPath (Join-Path $dir 'identity.secret') -ErrorAction SilentlyContinue).Length -gt 0
    if (!$ready) { Start-Sleep -Milliseconds 250 }
} until ($ready -or [DateTime]::UtcNow -ge $deadline)
if (!$ready) { throw 'Служба ZeroTier ещё не подготовила локальную идентичность. Повторите установку.' }
Assert-PrivateDirectory $dir
Assert-RegularPath $planetPath
if ([Convert]::ToBase64String([IO.File]::ReadAllBytes($planetPath)) -cne $info.planet) { throw $foreign }
if ($needsMsi) {
    $receipt = Join-Path $dir '.uplink-owner'
    Assert-RegularPath $receipt
    [IO.File]::WriteAllText($receipt,$who,[Text.Encoding]::ASCII)
    Remove-Item -LiteralPath $marker
}
foreach ($file in 'identity.secret','metricstoken.secret','authtoken.secret') {
    $target = Join-Path $dir $file
    if ($file -eq 'metricstoken.secret' -and !(Test-Path -LiteralPath $target -PathType Leaf)) { continue }
    Set-SecretAccess $target ($file -eq 'authtoken.secret')
}
& (Join-Path $PSScriptRoot 'transport/windows/Provision-DeviceProof.ps1') -ZeroTierHome $dir -UserSid $who
Write-Output 'Транспорт готов. Откройте приглашение в Uplink.'
