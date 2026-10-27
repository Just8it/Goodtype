param([Parameter(Mandatory = $true)][string]$Manifest)

$ErrorActionPreference = "Stop"
$manifestPath = (Resolve-Path -LiteralPath $Manifest).Path
$metadata = Get-Content -LiteralPath $manifestPath -Raw | ConvertFrom-Json
$packageRoot = Split-Path -Parent $manifestPath
$archive = Join-Path $packageRoot $metadata.archive
if ((Get-FileHash -LiteralPath $archive -Algorithm SHA256).Hash.ToLowerInvariant() -ne $metadata.sha256) {
    throw "Archive checksum differs from its manifest."
}

# Fresh relocation, rather than testing the build directory's DLL/path environment.
$temporaryRoot = [IO.Path]::GetFullPath([IO.Path]::GetTempPath())
$extracted = Join-Path $temporaryRoot ("goodtype-package-check-" + [guid]::NewGuid().ToString("N"))
try {
    Expand-Archive -LiteralPath $archive -DestinationPath $extracted
    foreach ($file in $metadata.files.PSObject.Properties) {
        $path = Join-Path $extracted $file.Name
        if ((Get-FileHash -LiteralPath $path -Algorithm SHA256).Hash.ToLowerInvariant() -ne $file.Value) {
            throw "Extracted file differs from its manifest: $($file.Name)"
        }
    }
    $version = (Get-Item -LiteralPath (Join-Path $extracted "Goodtype.exe")).VersionInfo.ProductVersion
    if ($version -ne $metadata.version) { throw "App version '$version' differs from '$($metadata.version)'." }

# The documented empty codec-1 binary layer exercises the shipped decoder without user files.
    $fixture = Join-Path $extracted "empty.ink"
    $inkMetadata = [Text.Encoding]::UTF8.GetBytes('{"schemaVersion":3,"id":"layer-1","pageId":"page-1","strokes":[]}')
    $header = [byte[]](0x47, 0x54, 0x49, 0x4e, 0x4b, 0, 0x0d, 0x0a) +
        [BitConverter]::GetBytes([uint32]1) + [BitConverter]::GetBytes([uint32]$inkMetadata.Length) +
        [BitConverter]::GetBytes([uint32]0)
    [IO.File]::WriteAllBytes($fixture, [byte[]]($header + $inkMetadata))
    $result = & (Join-Path $extracted "goodtype-ink.exe") decode $fixture
    if ($LASTEXITCODE -ne 0) { throw "Relocated ink decoder failed." }
    $decoded = $result | ConvertFrom-Json
    if ($decoded.strokes.Count -ne 0) { throw "Empty ink layer did not decode correctly." }
    Write-Host "Package hashes, app version, required files and relocated ink decoder passed."
}
finally {
    if (Test-Path -LiteralPath $extracted) {
        $resolvedExtraction = (Resolve-Path -LiteralPath $extracted).Path
        $expectedParent = $temporaryRoot.TrimEnd('\', '/')
        if ((Split-Path -Parent $resolvedExtraction) -ne $expectedParent -or
            (Split-Path -Leaf $resolvedExtraction) -notmatch '^goodtype-package-check-[0-9a-f]{32}$') {
            throw "Refusing cleanup outside this check's temporary extraction: $resolvedExtraction"
        }
        Remove-Item -LiteralPath $resolvedExtraction -Recurse -Force
    }
}
