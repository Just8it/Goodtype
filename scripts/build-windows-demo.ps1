$ErrorActionPreference = "Stop"

$repositoryRoot = (Resolve-Path (Join-Path $PSScriptRoot "..")).Path

# The Typst compiler is embedded in the app; the demo no longer ships typst.exe.

Push-Location $repositoryRoot
try {
    & pnpm install --frozen-lockfile
    if ($LASTEXITCODE -ne 0) { throw "pnpm install failed." }

    & pnpm tauri build
    if ($LASTEXITCODE -ne 0) { throw "Goodtype release build failed." }

    & cargo build --release -p goodtype-core --bin goodtype-ink
    if ($LASTEXITCODE -ne 0) { throw "Ink decoder release build failed." }
}
finally {
    Pop-Location
}

$releaseExecutable = Join-Path $repositoryRoot "target\release\goodtype-desktop.exe"
if (-not (Test-Path -LiteralPath $releaseExecutable)) {
    throw "The release executable was not created at target\release\goodtype-desktop.exe."
}

$sidecar = Join-Path $repositoryRoot "target\release\tinymist.exe"
$decoder = Join-Path $repositoryRoot "target\release\goodtype-ink.exe"
foreach ($requiredFile in @($sidecar, $decoder)) {
    if (-not (Test-Path -LiteralPath $requiredFile -PathType Leaf)) {
        throw "Required release binary is missing: $requiredFile"
    }
}

$packageName = "Goodtype-Windows-" + (Get-Date -Format "yyyyMMdd-HHmmss")
$demoDirectory = Join-Path $repositoryRoot "target\$packageName"
$zipPath = "$demoDirectory.zip"

New-Item -ItemType Directory -Path $demoDirectory | Out-Null
Copy-Item -LiteralPath $releaseExecutable -Destination (Join-Path $demoDirectory "Goodtype.exe")
Copy-Item -LiteralPath $sidecar -Destination (Join-Path $demoDirectory "tinymist.exe")
Copy-Item -LiteralPath $decoder -Destination (Join-Path $demoDirectory "goodtype-ink.exe")
Copy-Item -LiteralPath (Join-Path $repositoryRoot "formats\ink-v1.md") -Destination $demoDirectory

@'
@echo off
start "" "%~dp0Goodtype.exe"
'@ | Set-Content -LiteralPath (Join-Path $demoDirectory "Start Goodtype.cmd") -Encoding Ascii

@'
Goodtype for Windows x64

Extract this entire ZIP to a folder, then open Goodtype.exe.
Keep tinymist.exe beside Goodtype.exe for editor intelligence.
Microsoft Edge WebView2 Runtime is required by the desktop app.

This development build writes schema-3 notebooks. Older Goodtype versions
cannot open that schema. Legacy ink originals are retained during migration.

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
Write-Host "Created $zipPath"
