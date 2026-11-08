param(
    [ValidateSet("local", "dev", "stable")]
    [string]$Channel = "local"
)

$ErrorActionPreference = "Stop"

$repositoryRoot = (Resolve-Path (Join-Path $PSScriptRoot "..")).Path

# The Typst compiler is embedded in the app; the demo no longer ships typst.exe.

Push-Location $repositoryRoot
try {
    & node scripts/release.mjs prepare $Channel
    if ($LASTEXITCODE -ne 0) { throw "Release metadata validation failed." }
    $metadata = Get-Content -LiteralPath "target/release-metadata.json" -Raw | ConvertFrom-Json
    $demoDirectory = Join-Path $repositoryRoot "target\$($metadata.packageName)"
    $zipPath = "$demoDirectory.zip"
    if ((Test-Path -LiteralPath $demoDirectory) -or (Test-Path -LiteralPath $zipPath)) {
        throw "This package already exists. Never replace an existing build: $demoDirectory"
    }

    & pnpm.cmd install --frozen-lockfile
    if ($LASTEXITCODE -ne 0) { throw "pnpm install failed." }

    & pnpm.cmd tauri build --config (Join-Path $repositoryRoot "target/release-config.json") -- --locked
    if ($LASTEXITCODE -ne 0) { throw "Goodtype release build failed." }

    & cargo build --release --locked -p goodtype-core --bin goodtype-ink
    if ($LASTEXITCODE -ne 0) { throw "Ink decoder release build failed." }

    & cargo build --release --locked -p goodtype-mcp
    if ($LASTEXITCODE -ne 0) { throw "AI relay release build failed." }
}
finally {
    Pop-Location
}

$buildTargetRoot = if ($env:CARGO_TARGET_DIR) {
    if ([IO.Path]::IsPathRooted($env:CARGO_TARGET_DIR)) {
        [IO.Path]::GetFullPath($env:CARGO_TARGET_DIR)
    } else {
        [IO.Path]::GetFullPath((Join-Path $repositoryRoot $env:CARGO_TARGET_DIR))
    }
} else {
    Join-Path $repositoryRoot "target"
}
$releaseExecutable = Join-Path $buildTargetRoot "release\goodtype-desktop.exe"
if (-not (Test-Path -LiteralPath $releaseExecutable)) {
    throw "The release executable was not created at $releaseExecutable."
}

$sidecar = Join-Path $buildTargetRoot "release\tinymist.exe"
$decoder = Join-Path $buildTargetRoot "release\goodtype-ink.exe"
$relay = Join-Path $buildTargetRoot "release\goodtype-mcp.exe"
foreach ($requiredFile in @($sidecar, $decoder, $relay)) {
    if (-not (Test-Path -LiteralPath $requiredFile -PathType Leaf)) {
        throw "Required release binary is missing: $requiredFile"
    }
}

New-Item -ItemType Directory -Path $demoDirectory | Out-Null
Copy-Item -LiteralPath $releaseExecutable -Destination (Join-Path $demoDirectory "Goodtype.exe")
Copy-Item -LiteralPath $sidecar -Destination (Join-Path $demoDirectory "tinymist.exe")
Copy-Item -LiteralPath $decoder -Destination (Join-Path $demoDirectory "goodtype-ink.exe")
Copy-Item -LiteralPath $relay -Destination (Join-Path $demoDirectory "goodtype-mcp.exe")
Copy-Item -LiteralPath (Join-Path $repositoryRoot "crates\goodtype-mcp\skill") -Destination $demoDirectory -Recurse
Copy-Item -LiteralPath (Join-Path $repositoryRoot "formats\ink-v1.md") -Destination $demoDirectory
foreach ($notice in @("LICENSE", "NOTICE")) {
    Copy-Item -LiteralPath (Join-Path $repositoryRoot $notice) -Destination $demoDirectory
}

@'
Goodtype for Windows x64

Extract this entire ZIP to a folder, then open Goodtype.exe.
Keep tinymist.exe beside Goodtype.exe for editor intelligence.
Keep goodtype-mcp.exe and the skill folder beside it for optional AI assistance.
Microsoft Edge WebView2 Runtime is required by the desktop app.

This build supports schema-4 notebooks with computational pages. Older
Goodtype versions cannot open schema 4. Normal notebooks retain schema 3
until computational content is added. Legacy ink originals are preserved.

To decode an ink file to JSON from PowerShell in this folder:
  .\goodtype-ink.exe decode "C:\path\layer.ink"
The decoder prints JSON to the terminal. It does not modify the notebook.
The binary format is documented in ink-v1.md.
'@ | Set-Content -LiteralPath (Join-Path $demoDirectory "README.txt") -Encoding Ascii

for ($archiveAttempt = 1; $archiveAttempt -le 3; $archiveAttempt++) {
    try {
        # The package directory is fresh; replace only this build's partial ZIP after a scan lock.
        Compress-Archive -Path (Join-Path $demoDirectory "*") -DestinationPath $zipPath -Force
        break
    }
    catch {
        if ($archiveAttempt -eq 3) { throw }
        Start-Sleep -Seconds 1
    }
}
Push-Location $repositoryRoot
try {
    & node scripts/release.mjs manifest
    if ($LASTEXITCODE -ne 0) { throw "Release manifest creation failed." }
    & (Join-Path $PSScriptRoot "test-windows-package.ps1") -Manifest "$demoDirectory.json"
}
finally {
    Pop-Location
}
Write-Host "Created $demoDirectory"
Write-Host "Created $zipPath"
