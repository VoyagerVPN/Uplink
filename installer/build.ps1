param([ValidatePattern('^Uplink-QA-[a-z0-9]+$')][string]$TestProduct)
$ErrorActionPreference='Stop'
$repo=Split-Path -Parent $PSScriptRoot
Push-Location $repo
try {
    $sdk=Join-Path $repo '.cache/nsis'
    $out=Join-Path $repo '.cache/packages'
    New-Item -Type Directory -Force $sdk,$out | Out-Null
    $compiler=Join-Path $sdk 'nsis-3.13/makensis.exe'
    if (!(Test-Path -LiteralPath $compiler)) {
        $zip=Join-Path $sdk 'nsis-3.13.zip'
        $sha='BA63DFFC4410EE89193E1CB5A41989991BD77C61068DA17E3156D136B7B0B3D8'
        if (!(Test-Path $zip) -or (Get-FileHash $zip).Hash -ne $sha) {
            Invoke-WebRequest -UseBasicParsing 'https://downloads.sourceforge.net/project/nsis/NSIS%203/3.13/nsis-3.13.zip' -OutFile $zip
        }
        if ((Get-FileHash $zip).Hash -ne $sha) {throw 'NSIS archive checksum mismatch'}
        Expand-Archive -LiteralPath $zip -DestinationPath $sdk -Force
    }
    & .\dev.ps1 perf
    if ($LASTEXITCODE) {throw 'Uplink build failed'}
    $product=if ($TestProduct) {$TestProduct} else {'Uplink'}
    $protocol=if ($TestProduct) {$TestProduct} else {'uplink'}
    $version=([regex]::Match((Get-Content Cargo.toml -Raw),'(?m)^version = "(\d+\.\d+\.\d+)"')).Groups[1].Value
    $bin=Join-Path $repo '.cache/target/perf'
    $reader=[IO.BinaryReader]::new([IO.File]::OpenRead((Join-Path $bin 'uplink.exe')))
    try {
        $reader.BaseStream.Position=0x3c
        $reader.BaseStream.Position=$reader.ReadUInt32()+4
        if ($reader.ReadUInt16() -ne 0x8664) {throw 'Uplink requires an x64 build'}
    } finally {$reader.Dispose()}
    $helper=Join-Path $sdk 'UplinkNetwork.exe'
    & $compiler '/INPUTCHARSET' 'UTF8' '/V2' "/DOUTPUT=$helper" (Join-Path $PSScriptRoot 'network.nsi')
    if ($LASTEXITCODE) {throw 'Network helper build failed'}
    $inputs=@($helper,'installer/cleanup-user.ps1',"$bin/uplink.exe","$bin/uplink-window.exe","$bin/uplinkctl.exe",'install-transport.ps1','transport/windows/Provision-DeviceProof.ps1','assets/fonts/Inter-LICENSE.txt','installer/uplink.nsi')
    $hashes=($inputs | ForEach-Object {(Get-FileHash -LiteralPath $_).Hash}) -join ''
    $alg=[Security.Cryptography.SHA256]::Create()
    try {$tag=([BitConverter]::ToString($alg.ComputeHash([Text.Encoding]::ASCII.GetBytes($hashes)))).Replace('-','').Substring(0,20)} finally {$alg.Dispose()}
    $output=Join-Path $out "$product-Setup.exe"
    & $compiler '/INPUTCHARSET' 'UTF8' '/V2' "/DNETWORK_HELPER=$helper" "/DPRODUCT=$product" "/DPROTOCOL=$protocol" "/DVERSION=$version" "/DTAG=$tag" "/DBIN=$bin" "/DOUTPUT=$output" (Join-Path $PSScriptRoot 'uplink.nsi')
    if ($LASTEXITCODE) {throw 'NSIS build failed'}
    Write-Output $output
} finally {Pop-Location}
