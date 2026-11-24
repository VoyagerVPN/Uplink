param([Parameter(Mandatory)][ValidateSet('Install','Remove')][string]$Action,
      [Parameter(Mandatory)][ValidatePattern('^(Uplink|Uplink-QA-[a-z0-9]+)$')][string]$Product,
      [Parameter(Mandatory)][ValidatePattern('^[0-9A-F]{20}$')][string]$Tag)
$ErrorActionPreference='Stop'
$env:PSModulePath=Join-Path $PSHOME 'Modules'
$root=Join-Path ([Environment]::GetFolderPath('ProgramFiles')) $Product
$network=Join-Path $root 'networks'
$binary=Join-Path $root "packages\$Tag\uplink-network-service.exe"
$name=$Product+'Network'
$sc=Join-Path ([Environment]::SystemDirectory) 'sc.exe'
function Invoke-Control([string[]]$Arguments) {
    & $sc @Arguments | Out-Null
    if ($LASTEXITCODE) { throw "Service control failed ($LASTEXITCODE)" }
}
$service=Get-Service -Name $name -ErrorAction SilentlyContinue
$previous=$null
$running=$false
if ($service) {
    $previous=(Get-ItemProperty -LiteralPath "HKLM:\SYSTEM\CurrentControlSet\Services\$name").ImagePath
    $pattern='^"'+[regex]::Escape($root+'\packages\')+'[0-9A-F]{20}\\uplink-network-service\.exe"$'
    if ($previous -notmatch $pattern) { throw 'Service name belongs to another installation' }
    $running=$service.Status -ne 'Stopped'
    if ($running) {
        $service.Stop()
        $service.WaitForStatus('Stopped',[TimeSpan]::FromSeconds(30))
    }
    $service.Dispose()
}
if ($Action -eq 'Remove') {
    if ($previous) { Invoke-Control @('delete',$name) }
    $active=Join-Path $network 'active'
    foreach ($path in $root,$network,$active) {
        $dir=Get-Item -LiteralPath $path -Force -ErrorAction SilentlyContinue
        if ($dir -and ($dir.Attributes -band [IO.FileAttributes]::ReparsePoint)) { throw 'Unexpected redirected service directory' }
    }
    if ([IO.Directory]::Exists($active)) {
        foreach ($file in Get-ChildItem -LiteralPath $active -File -Force) {
            if ($file.Name -cmatch '^[0-9a-f]{32}\.toml$') { [IO.File]::Delete($file.FullName) }
        }
        if (!(Get-ChildItem -LiteralPath $active -Force)) { [IO.Directory]::Delete($active) }
    }
    return
}
if (!(Test-Path -LiteralPath $binary -PathType Leaf)) { throw 'Network service binary is missing' }
New-Item -ItemType Directory -Force $network | Out-Null
& "$env:SystemRoot\System32\icacls.exe" $network '/inheritance:r' '/grant:r' '*S-1-5-18:(OI)(CI)F' '*S-1-5-32-544:(OI)(CI)F' | Out-Null
if ($LASTEXITCODE) { throw 'Could not protect the network configuration' }
$verb=if ($previous) { 'config' } else { 'create' }
function Set-Binary([string]$Verb,[string]$Path) {
    $image='"'+$Path+'"'
    if ($Verb -eq 'create') {
        New-Service -Name $name -BinaryPathName $image -StartupType Manual -DisplayName 'Uplink Network Service' | Out-Null
    } else {
        $item=Get-CimInstance Win32_Service -Filter "Name='$name'"
        $result=Invoke-CimMethod -InputObject $item -MethodName Change -Arguments @{PathName=$image;StartMode='Manual'}
        if ($result.ReturnValue -ne 0) { throw "Could not update the service ($($result.ReturnValue))" }
    }
}
try {
    Set-Binary $verb $binary
    $registered=(Get-ItemProperty -LiteralPath "HKLM:\SYSTEM\CurrentControlSet\Services\$name").ImagePath
    if ($registered -cne ('"'+$binary+'"')) { throw 'Unexpected registered service executable' }
    Invoke-Control @('sdset',$name,'D:P(A;;GA;;;SY)(A;;GA;;;BA)(A;;LCRPWP;;;BU)')
    if ($running) { Start-Service -Name $name }
} catch {
    if ($previous) {
        Set-Binary 'config' $previous.Trim('"')
        if ($running) { Start-Service -Name $name }
    } elseif (Get-Service -Name $name -ErrorAction SilentlyContinue) {
        Invoke-Control @('delete',$name)
    }
    throw
}
