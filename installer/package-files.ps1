function Copy-ArchiveFiles([string]$Archive, [hashtable]$Files, [string]$Destination) {
    Add-Type -AssemblyName System.IO.Compression.FileSystem
    [IO.Directory]::CreateDirectory($Destination) | Out-Null
    $zip = [IO.Compression.ZipFile]::OpenRead($Archive)
    try {
        foreach ($name in $Files.Keys) {
            $entry = $zip.GetEntry($name)
            if (!$entry) { throw "Missing package file: $name" }
            $target = Join-Path $Destination $Files[$name]
            if (Test-Path -LiteralPath $target) {
                $stream = $entry.Open()
                try { $expected = (Get-FileHash -InputStream $stream).Hash } finally { $stream.Dispose() }
                if ((Get-FileHash -LiteralPath $target).Hash -eq $expected) { continue }
            }
            [IO.Compression.ZipFileExtensions]::ExtractToFile($entry, $target, $true)
        }
    } finally { $zip.Dispose() }
}
