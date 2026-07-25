$ErrorActionPreference = "Stop"

$repositoryRoot = (Resolve-Path (Join-Path $PSScriptRoot "..")).Path

# The Typst compiler is embedded in the app; the demo no longer ships typst.exe.

Push-Location $repositoryRoot
try {
    & pnpm install --frozen-lockfile
    if ($LASTEXITCODE -ne 0) { throw "pnpm install failed." }

    & pnpm tauri build
    if ($LASTEXITCODE -ne 0) { throw "Goodtype release build failed." }
}
finally {
    Pop-Location
}

$releaseExecutable = Join-Path $repositoryRoot "target\release\goodtype-desktop.exe"
if (-not (Test-Path -LiteralPath $releaseExecutable)) {
    throw "The release executable was not created at target\release\goodtype-desktop.exe."
}

$demoDirectory = Join-Path $repositoryRoot "target\Goodtype-demo"
$zipPath = Join-Path $repositoryRoot "target\Goodtype-demo.zip"

if (Test-Path -LiteralPath $demoDirectory) {
    Remove-Item -LiteralPath $demoDirectory -Recurse -Force
}
if (Test-Path -LiteralPath $zipPath) {
    Remove-Item -LiteralPath $zipPath -Force
}

New-Item -ItemType Directory -Path $demoDirectory | Out-Null
Copy-Item -LiteralPath $releaseExecutable -Destination (Join-Path $demoDirectory "Goodtype.exe")

@'
@echo off
start "" "%~dp0Goodtype.exe"
'@ | Set-Content -LiteralPath (Join-Path $demoDirectory "Start Goodtype.cmd") -Encoding Ascii

Compress-Archive -Path (Join-Path $demoDirectory "*") -DestinationPath $zipPath
Write-Host "Created $zipPath"
