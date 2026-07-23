$ErrorActionPreference = "Stop"

$repositoryRoot = (Resolve-Path (Join-Path $PSScriptRoot "..")).Path
$localTypst = Join-Path $repositoryRoot "target\tools\typst.exe"
$installedTypst = Get-Command typst -ErrorAction SilentlyContinue
$typstExecutable = if ($installedTypst) { $installedTypst.Source } elseif (Test-Path -LiteralPath $localTypst) { $localTypst } else { $null }

if (-not $typstExecutable) {
    throw "Typst was not found. Install Typst 0.15.1 or copy typst.exe to target\tools\typst.exe."
}

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
Copy-Item -LiteralPath $typstExecutable -Destination (Join-Path $demoDirectory "typst.exe")

@'
@echo off
set "GOODTYPE_TYPST_BIN=%~dp0typst.exe"
start "" "%~dp0Goodtype.exe"
'@ | Set-Content -LiteralPath (Join-Path $demoDirectory "Start Goodtype.cmd") -Encoding Ascii

Compress-Archive -Path (Join-Path $demoDirectory "*") -DestinationPath $zipPath
Write-Host "Created $zipPath"
