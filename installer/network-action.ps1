$ErrorActionPreference = 'Stop'
$env:PSModulePath=Join-Path $PSHOME 'Modules'
if ($env:UPLINK_NETWORK_ACTION -notmatch '^(prepare|clean|remove) (S-1-[0-9-]+)(?: ([0-9a-f]{32}))?$') {throw 'Invalid action'}
$action,$ownerSid,$request = $Matches[1],$Matches[2],$Matches[3]
if ([Security.Principal.WindowsIdentity]::GetCurrent().User.Value -ne $ownerSid) {exit 37}
if ($action -eq 'prepare') {
    if (!$request) {throw 'Missing invitation'}
    $file=Join-Path ([Environment]::GetFolderPath('LocalApplicationData')) "Uplink\requests\$request"
    $reader=[IO.File]::OpenRead($file)
    try {
        if ($reader.Length -gt 512) {throw 'Invalid invitation'}
        $inputText=[IO.StreamReader]::new($reader)
        $invitation=$inputText.ReadToEnd()
    } finally {$reader.Dispose()}
    $env:ProgramData=[Environment]::GetFolderPath('CommonApplicationData')
    $env:SystemRoot=[IO.Directory]::GetParent([Environment]::SystemDirectory).FullName
    try { & (Join-Path $PSScriptRoot 'install-transport.ps1') -Invitation $invitation }
    catch {
        $code = $_.Exception.Data['UplinkSetupCode']
        if ($code -in (31..36)) { exit $code }
        throw
    }
    return
}
. (Join-Path $PSScriptRoot 'remove-network.ps1')
$data=[Environment]::GetFolderPath('CommonApplicationData')
try {Remove-NetworkComponents (Join-Path $data 'ZeroTier') $ownerSid $action}
catch {if ($_.Exception.Message -eq 'RESTART_REQUIRED') {exit 29}; throw}
