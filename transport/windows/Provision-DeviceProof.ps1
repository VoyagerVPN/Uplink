param(
    [Parameter(Mandatory)][string]$ZeroTierHome,
    [Parameter(Mandatory)][string]$UserSid
)
$ErrorActionPreference = 'Stop'
$system = [Security.Principal.SecurityIdentifier]::new('S-1-5-18')
$admins = [Security.Principal.SecurityIdentifier]::new('S-1-5-32-544')
$user = [Security.Principal.SecurityIdentifier]::new($UserSid)
$machineWriters = @($system.Value, $admins.Value)

function Assert-MachinePath([string]$Path) {
    $item = Get-Item -Force -LiteralPath $Path
    if ($item.Attributes -band [IO.FileAttributes]::ReparsePoint) {
        throw "Путь настройки подписи содержит перенаправление."
    }
    $acl = Get-Acl -LiteralPath $Path
    if ($acl.GetOwner([Security.Principal.SecurityIdentifier]).Value -notin $machineWriters) {
        throw 'Владелец файлов подписи должен быть SYSTEM или Администраторы.'
    }
    # Запись данных, атрибутов и ACL, удаление или замена объекта.
    $unsafe = 0xD0156
    foreach ($entry in $acl.GetAccessRules($true, $true, [Security.Principal.SecurityIdentifier])) {
        if ($entry.AccessControlType -eq 'Allow' -and
            $entry.IdentityReference.Value -notin $machineWriters -and
            ([int]$entry.FileSystemRights -band $unsafe)) {
            throw 'Каталог или файлы подписи доступны другим пользователям для записи.'
        }
    }
}
function Assert-SecretReaders([string]$Path, [string[]]$Trusted) {
    $acl = Get-Acl -LiteralPath $Path
    foreach ($entry in $acl.GetAccessRules($true, $true, [Security.Principal.SecurityIdentifier])) {
        if ($entry.AccessControlType -eq 'Allow' -and
            $entry.IdentityReference.Value -notin $Trusted -and
            ([int]$entry.FileSystemRights -band 1)) {
            throw 'Закрытый ключ доступен для чтения посторонней учётной записи.'
        }
    }
}
function Invoke-IdTool([string[]]$Arguments) {
    $info = [Diagnostics.ProcessStartInfo]::new()
    $info.FileName = $binary
    $info.UseShellExecute = $false
    $info.CreateNoWindow = $true
    $info.RedirectStandardOutput = $true
    $info.RedirectStandardError = $true
    $info.Arguments = (($Arguments | ForEach-Object { '"' + $_ + '"' }) -join ' ')
    $process = [Diagnostics.Process]::Start($info)
    try {
        $stdout = $process.StandardOutput.ReadToEndAsync()
        $stderr = $process.StandardError.ReadToEndAsync()
        if (!$process.WaitForExit(30000)) {
            $process.Kill()
            throw 'Истекло время операции с ключом Uplink.'
        }
        if ($process.ExitCode -ne 0) { throw 'ZeroTier не подтвердил ключ устройства.' }
        return $stdout.GetAwaiter().GetResult().Trim()
    } finally { $process.Dispose() }
}

$binary = Join-Path $ZeroTierHome 'zerotier-one_x64.exe'
$nodeKey = Join-Path $ZeroTierHome 'identity.secret'
$folder = Join-Path $ZeroTierHome 'uplink-proof'
foreach ($item in $ZeroTierHome,$binary,$nodeKey) { Assert-MachinePath $item }
Assert-SecretReaders $nodeKey $machineWriters
if (!(Test-Path -LiteralPath $folder)) {
    [IO.Directory]::CreateDirectory($folder) | Out-Null
}
Assert-MachinePath $folder
$acl = [Security.AccessControl.DirectorySecurity]::new()
$acl.SetOwner($admins)
$acl.SetAccessRuleProtection($true,$false)
foreach ($sid in $system,$admins,$user) {
    $rights = if ($sid.Value -in $machineWriters) { 'FullControl' } else { 'ReadAndExecute' }
    $rule = [Security.AccessControl.FileSystemAccessRule]::new(
        $sid, $rights, 'ContainerInherit,ObjectInherit', 'None', 'Allow')
    $acl.AddAccessRule($rule)
}
Set-Acl -LiteralPath $folder -AclObject $acl
foreach ($child in (Get-ChildItem -LiteralPath $folder -Force)) {
    Assert-MachinePath $child.FullName
}
$lock = [IO.File]::Open((Join-Path $folder 'provision.lock'), 'OpenOrCreate', 'ReadWrite', 'None')
try {
    $private = Join-Path $folder 'delegate.secret'
    $public = Join-Path $folder 'delegate.public'
    $binding = Join-Path $folder 'binding'
    $certificate = Join-Path $folder 'certificate'
    if (!(Test-Path -LiteralPath $private)) {
        $pending = Join-Path $folder 'delegate.next'
        Invoke-IdTool @('-i','generate',$pending) | Out-Null
        [IO.File]::Move($pending,$private)
    }
    Assert-SecretReaders $private ($machineWriters + $user.Value)
    $delegate = Invoke-IdTool @('-i','getpublic',$private)
    if ($delegate -cnotmatch '^[0-9a-f]{10}:0:[0-9a-f]{128}$') { throw 'Некорректный публичный ключ Uplink.' }
    [IO.File]::WriteAllText($public, $delegate, [Text.Encoding]::ASCII)
    [IO.File]::WriteAllText($binding, ("uplink/delegate/v1`n" + $delegate), [Text.Encoding]::ASCII)
    $signature = Invoke-IdTool @('-i','sign',$nodeKey,$binding)
    if ($signature -cnotmatch '^[0-9a-f]{192}$') { throw 'Не удалось подписать ключ Uplink.' }
    Invoke-IdTool @('-i','verify',$nodeKey,$binding,$signature) | Out-Null
    [IO.File]::WriteAllText($certificate,$signature,[Text.Encoding]::ASCII)
} finally {
    $lock.Dispose()
}

