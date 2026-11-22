$ErrorActionPreference='Stop'
$repo=Split-Path $PSScriptRoot
. (Join-Path $repo 'installer/remove-network.ps1')
$base=Join-Path $repo ('.cache/zt-cleanup-'+[guid]::NewGuid().ToString('N'))
$sid=[Security.Principal.WindowsIdentity]::GetCurrent().User.Value
$admin=[Security.Principal.SecurityIdentifier]::new('S-1-5-32-544')
$sys=[Security.Principal.SecurityIdentifier]::new('S-1-5-18')
$rules=[Security.AccessControl.DirectorySecurity]::new()
$rules.SetOwner($admin)
$rules.SetAccessRuleProtection($true,$false)
foreach ($id in $sys,$admin) {$rules.AddAccessRule([Security.AccessControl.FileSystemAccessRule]::new($id,'FullControl','ContainerInherit,ObjectInherit','None','Allow'))}
$rules.AddAccessRule([Security.AccessControl.FileSystemAccessRule]::new([Security.Principal.SecurityIdentifier]::new($sid),'ReadAndExecute','ContainerInherit,ObjectInherit','None','Allow'))
if ($PSVersionTable.PSEdition -eq 'Core') {[IO.FileSystemAclExtensions]::CreateDirectory($rules,$base) | Out-Null}
else {[IO.Directory]::CreateDirectory($base,$rules) | Out-Null}
$dir=New-Item -Type Directory (Join-Path $base 'One')
$proof=New-Item -Type Directory (Join-Path $dir.FullName 'uplink-proof')
[IO.File]::WriteAllText((Join-Path $proof.FullName 'delegate.secret'),'test')
[IO.File]::WriteAllText((Join-Path $dir.FullName '.uplink-owner'),$sid)
[IO.File]::WriteAllText((Join-Path $dir.FullName 'planet'),'test')
$id='{00000000-0000-0000-0000-000000000001}'
function Get-ZeroTierProducts {$id}
$state=@{calls=0;exit=1603}
function Start-Process($FilePath,$ArgumentList,$WindowStyle,[switch]$Wait,[switch]$PassThru) {
    if ((Split-Path $FilePath -Leaf) -ne 'msiexec.exe' -or $ArgumentList -ne "/x $id /qn /norestart") {throw 'Unexpected uninstall invocation'}
    $state.calls++
    @{ExitCode=$state.exit}
}
Remove-NetworkComponents $base 'S-1-5-1234' 'clean'
if (!(Test-Path $proof.FullName)) {throw 'Removed another account proof'}
Remove-NetworkComponents $base $sid 'clean'
if ((Test-Path $proof.FullName) -or (Test-Path (Join-Path $dir.FullName '.uplink-owner')) -or !(Test-Path (Join-Path $dir.FullName 'planet')) -or $state.calls) {throw 'Shared transport cleanup failed'}
try {Remove-NetworkComponents $base $sid 'remove';throw 'Failed MSI accepted'} catch {if ($_.Exception.Message -ne 'MSI 1603') {throw}}
if (!(Test-Path -LiteralPath $dir.FullName)) {throw 'Removed data after MSI failure'}
$state.exit=0
Remove-NetworkComponents $base $sid 'remove'
if ((Test-Path -LiteralPath $base) -or $state.calls -ne 2) {throw 'Incomplete cleanup after MSI success'}
'Shared transport, other owner, MSI failure and full removal OK'
