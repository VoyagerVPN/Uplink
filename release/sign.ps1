param([string]$Setup, [string]$Client, [string]$Version, [string]$Notes, [string]$Output, [switch]$Initialize)
$ErrorActionPreference = 'Stop'
$name = 'VoyagerVPN.Uplink.Release.v1'
$provider = [Security.Cryptography.CngProvider]::MicrosoftSoftwareKeyStorageProvider
$publicFile = Join-Path $PSScriptRoot 'public-key.txt'
if (![Security.Cryptography.CngKey]::Exists($name,$provider)) {
    if (!$Initialize -or (Test-Path $publicFile)) { throw 'Original Uplink signing key is required on this publisher account.' }
    $parameters = [Security.Cryptography.CngKeyCreationParameters]::new()
    $parameters.Provider = $provider
    $parameters.KeyUsage = [Security.Cryptography.CngKeyUsages]::Signing
    $parameters.ExportPolicy = [Security.Cryptography.CngExportPolicies]::None
    $key = [Security.Cryptography.CngKey]::Create([Security.Cryptography.CngAlgorithm]::ECDsaP256,$name,$parameters)
} else { $key = [Security.Cryptography.CngKey]::Open($name,$provider) }
$signer = [Security.Cryptography.ECDsaCng]::new($key)
try {
    $xy = $key.Export([Security.Cryptography.CngKeyBlobFormat]::EccPublicBlob)
    $public = [Convert]::ToBase64String([byte[]](@(4) + $xy[8..71]))
    if ($Initialize -and !(Test-Path $publicFile)) { [IO.File]::WriteAllText($publicFile,$public) }
    if ([IO.File]::ReadAllText($publicFile).Trim() -cne $public) { throw 'Signing key differs from the key trusted by Uplink.' }
    if ($Initialize) { return }
    if ($Version -cnotmatch '^\d+\.\d+\.\d+$' -or (Get-Item -LiteralPath $Setup).VersionInfo.FileVersion -cne $Version) {throw 'Installer version mismatch'}
    $manifest = [ordered]@{product='VoyagerVPN/Uplink'; version=$Version; size=(Get-Item -LiteralPath $Setup).Length;
        sha256=(Get-FileHash -LiteralPath $Setup).Hash.ToLowerInvariant(); client_sha256=(Get-FileHash -LiteralPath $Client).Hash.ToLowerInvariant();
        notes=[IO.File]::ReadAllText((Resolve-Path -LiteralPath $Notes)).Trim()}
    $payload = [Text.Encoding]::UTF8.GetBytes(($manifest | ConvertTo-Json -Compress))
    $sig = $signer.SignData($payload,[Security.Cryptography.HashAlgorithmName]::SHA256)
    $envelope = @{payload=[Convert]::ToBase64String($payload);signature=[Convert]::ToBase64String($sig)} | ConvertTo-Json -Compress
    [IO.File]::WriteAllText($Output,$envelope,[Text.UTF8Encoding]::new($false))
} finally { $signer.Dispose(); $key.Dispose() }

