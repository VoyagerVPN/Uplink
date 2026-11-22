$ErrorActionPreference = 'Stop'
Import-Module Microsoft.PowerShell.Security -ErrorAction Stop
$project = Split-Path $PSScriptRoot
$parseErrors = $null
$proofScript = Join-Path $project 'transport/windows/Provision-DeviceProof.ps1'
# Windows PowerShell 5.1 читает скрипты без BOM в системной кодировке.
$source = [IO.File]::ReadAllText($proofScript, [Text.Encoding]::GetEncoding(1251))
$null = [System.Management.Automation.Language.Parser]::ParseInput($source, [ref]$null, [ref]$parseErrors)
if ($parseErrors.Count) { throw 'The device proof script does not parse in the installer PowerShell host' }
$cache = Join-Path $project '.cache'
$testRoot = Join-Path $cache ('installer-test-' + [guid]::NewGuid().ToString('N'))
$oldData = $env:ProgramData
$invitation = 'uplink://example.org/invite/' + ('a' * 64)
$testPlanet = [Convert]::ToBase64String([byte[]](1..100))
function Assert($result, $message) { if (!$result) { throw $message } }
function Invoke-RestMethod {
    param($Method,$Uri,$ContentType,$Body,$MaximumRedirection)
    $request = $Body | ConvertFrom-Json
    Assert ($Method -eq 'Post' -and $Uri -eq 'https://example.org/v1/bootstrap' -and
        $request.token -ceq ('a' * 64) -and $request.version -eq 2) 'Invalid bootstrap request'
    @{planet=$testPlanet}
}
function Invoke-WebRequest {
    param($Uri,$OutFile)
    [IO.File]::WriteAllText($OutFile,'MSI fixture')
}
function Get-AuthenticodeSignature {
    param($FilePath)
    $state.verifiedMsi = [IO.Path]::GetFullPath($FilePath)
    @{Status='Valid';SignerCertificate=@{Subject='CN="ZEROTIER, INC."'}}
}
function Get-Service {
    param($Name,$ErrorAction)
    $state.service
}
function Start-Service {
    param($Name)
    $state.service.Status = 'Running'
}
function Start-Process {
    param($FilePath,$ArgumentList,$WindowStyle,[switch]$Wait,[switch]$PassThru)
    Assert ((Split-Path -Leaf $FilePath) -eq 'msiexec.exe' -and $WindowStyle -eq 'Hidden') 'Unexpected external process'
    Assert (($ArgumentList -contains 'ZTHEADLESS=Yes') -eq [bool]$state.headless) 'Unexpected MSI desktop UI mode'
    $installMsi = [IO.Path]::GetFullPath($ArgumentList[1].Trim('"'))
    $trustedBase = [IO.Path]::GetFullPath((Join-Path $env:ProgramData 'ZeroTier')) + [IO.Path]::DirectorySeparatorChar
    Assert ($installMsi -eq $state.verifiedMsi -and $installMsi.StartsWith($trustedBase,[StringComparison]::OrdinalIgnoreCase)) 'MSI path remains writable after signature verification'
    foreach ($secret in 'identity.secret','authtoken.secret','metricstoken.secret') {
        $target = Join-Path $ztDir $secret
        $acl = Get-Acl -LiteralPath $target
        $grants = $acl.GetAccessRules($true,$true,[Security.Principal.SecurityIdentifier]).IdentityReference.Value
        Assert ($acl.AreAccessRulesProtected -and 'S-1-1-0' -notin $grants -and 'S-1-5-32-545' -notin $grants) 'Unprotected secret at MSI start'
        if ($secret -ne 'identity.secret') { Assert ((Get-Item -LiteralPath $target).Length -ge 32) 'Empty service credential' }
    }
    $state.installCalls++
    if ($state.msiExit) { $state.service = @{Status='Stopped'} }
    if (!$state.msiExit) {
        $state.service = @{Status='Running'}
        foreach ($secret in 'authtoken.secret','identity.secret','identity.public','zerotier-one_x64.exe') {
            [IO.File]::WriteAllText((Join-Path $ztDir $secret), 'fixture')
        }
        $newIdentity = Get-Acl -LiteralPath (Join-Path $ztDir 'identity.secret')
        $readers = $newIdentity.GetAccessRules($true,$true,[Security.Principal.SecurityIdentifier]).IdentityReference.Value
        Assert ('S-1-1-0' -notin $readers -and 'S-1-5-32-545' -notin $readers) 'Unprotected node identity during installation'
    }
    @{ExitCode=$state.msiExit}
}
function Run-Installer {
    $before = [Console]::In
    try {
        [Console]::SetIn([IO.StringReader]::new($invitation))
        & (Join-Path $testRoot 'install-transport.ps1') -ReadInvitation -Headless:([bool]$state.headless)
    } finally { [Console]::SetIn($before) }
}
function Expect-Error([scriptblock]$action,[string]$pattern) {
    try { & $action } catch { if ($_.Exception.Message -notmatch $pattern) { throw }; return }
    throw "Expected error matching $pattern"
}
try {
    New-Item -ItemType Directory -Path (Join-Path $testRoot 'transport/windows') -Force | Out-Null
    Copy-Item (Join-Path $project 'install-transport.ps1') $testRoot
    $stub = 'param($ZeroTierHome,$UserSid)
        if ($ZeroTierHome -ne $ztDir -or !$UserSid) {throw "Incorrect proof setup arguments"}
        $state.proofs++
        if ($state.rejectProof) { throw "Proof setup interrupted" }'
    Set-Content -LiteralPath (Join-Path $testRoot 'transport/windows/Provision-DeviceProof.ps1') -Value $stub
    $state = @{proofs=0; rejectProof=$false; service=$null; msiExit=1603; installCalls=0}
    $env:ProgramData = Join-Path $testRoot 'ProgramData'
    $ztDir = Join-Path $env:ProgramData 'ZeroTier/One'
    New-Item -ItemType Directory -Path $env:ProgramData | Out-Null
    Expect-Error { Run-Installer } '1603'
    Assert ($state.installCalls -eq 1) 'Initial MSI attempt missing'
    $state.msiExit = 0
    $state.headless = $true
    $state.rejectProof = $true
    Expect-Error { Run-Installer } 'Proof setup interrupted'
    Assert ($state.installCalls -eq 2) 'Interrupted MSI setup was not retried'
    Assert ((Get-Content -Raw -LiteralPath (Join-Path $ztDir '.uplink-owner')) -ceq [Security.Principal.WindowsIdentity]::GetCurrent().User.Value) 'Missing installation receipt for ZeroTier'
    $identityPath = Join-Path $ztDir 'identity.secret'
    $unsafe = Get-Acl -LiteralPath $identityPath
    $publicSid = [Security.Principal.SecurityIdentifier]::new('S-1-1-0')
    $unsafe.AddAccessRule([Security.AccessControl.FileSystemAccessRule]::new($publicSid,'Read','Allow'))
    Set-Acl -LiteralPath $identityPath -AclObject $unsafe
    $state.service.Status = 'Stopped'
    $state.rejectProof = $false
    Run-Installer
    Assert ($state.service.Status -eq 'Running' -and $state.installCalls -eq 2 -and $state.proofs -eq 2) 'Failed to resume finalization'
    $privateAcl = Get-Acl -LiteralPath $identityPath
    $readers = $privateAcl.GetAccessRules($true,$true,[Security.Principal.SecurityIdentifier]).IdentityReference.Value
    Assert ($privateAcl.AreAccessRulesProtected -and 'S-1-1-0' -notin $readers) 'Retry left a public read rule on node identity'
    $tokenAcl = Get-Acl -LiteralPath (Join-Path $ztDir 'authtoken.secret')
    $grants = $tokenAcl.GetAccessRules($true,$true,[Security.Principal.SecurityIdentifier])
    $userSid = [Security.Principal.WindowsIdentity]::GetCurrent().User.Value
    Assert ($grants.Where({ $_.IdentityReference.Value -eq $userSid -and $_.AccessControlType -eq 'Allow' }).Count -eq 1) 'Client cannot read its local API token'
    $expected = [IO.File]::ReadAllBytes((Join-Path $ztDir 'planet'))
    $testPlanet = [Convert]::ToBase64String([byte[]](2..101))
    Expect-Error { Run-Installer } 'друг|совпад'
    Assert ([Convert]::ToBase64String($expected) -ceq [Convert]::ToBase64String([IO.File]::ReadAllBytes((Join-Path $ztDir 'planet')))) 'Overwrote foreign root'
    $state.service = $null
    $env:ProgramData = Join-Path $testRoot 'foreign'
    $ztDir = Join-Path $env:ProgramData 'ZeroTier/One'
    New-Item -ItemType Directory -Path $ztDir -Force | Out-Null
    [IO.File]::WriteAllBytes((Join-Path $ztDir 'planet'),[Convert]::FromBase64String($testPlanet))
    Expect-Error { Run-Installer } 'друг|чуж|сторон'
    Assert ($state.installCalls -eq 2) 'Installed over an unowned directory'
    'Bootstrap, MSI retry, finalization retry, foreign root and directory OK'
} finally {
    $env:ProgramData = $oldData
    $full = [IO.Path]::GetFullPath($testRoot)
    if ([IO.Path]::GetDirectoryName($full) -eq [IO.Path]::GetFullPath($cache) -and
        [IO.Path]::GetFileName($full) -match '^installer-test-[a-f0-9]{32}$' -and (Test-Path -LiteralPath $full)) {
        Remove-Item -LiteralPath $full -Recurse -Force
    }
}
