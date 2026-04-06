#requires -Version 5.1
<#
  Запуск симулятора P2P-сети (zona-p2p-sim) с CARGO_TARGET_DIR вне кириллического пути.

  Использование (из корня репозитория):
    .\scripts\sim-p2p.ps1
    .\scripts\sim-p2p.ps1 -DebugBuild
#>
param(
    [switch]$DebugBuild,
    [Parameter(ValueFromRemainingArguments = $true)]
    [string[]]$RunArgs
)

$ErrorActionPreference = "Stop"

$RepoRoot = Split-Path -Parent $PSScriptRoot
$P2pDir = Join-Path $RepoRoot "zona-p2p"
if (-not (Test-Path (Join-Path $P2pDir "Cargo.toml"))) {
    Write-Error "Не найден zona-p2p/Cargo.toml. Запустите из корня репозитория."
}

$isWin = [System.Runtime.InteropServices.RuntimeInformation]::IsOSPlatform(
    [System.Runtime.InteropServices.OSPlatform]::Windows)

if ($isWin) {
    $localApp = $env:LOCALAPPDATA
    if ([string]::IsNullOrEmpty($localApp)) { Write-Error "LOCALAPPDATA не задан." }
    $TargetRoot = Join-Path $localApp "Zona\cargo-target-p2p"
    $msysMingwBin = "C:\msys64\mingw64\bin"
    if (Test-Path (Join-Path $msysMingwBin "dlltool.exe")) {
        $env:Path = "${msysMingwBin};${env:Path}"
    }
} else {
    $homeDir = $env:HOME
    if ([string]::IsNullOrEmpty($homeDir)) { Write-Error "HOME не задан." }
    $TargetRoot = Join-Path $homeDir ".cache/zona-cargo-target-p2p"
}

New-Item -ItemType Directory -Force $TargetRoot | Out-Null
$env:CARGO_TARGET_DIR = $TargetRoot
Write-Host "CARGO_TARGET_DIR=$TargetRoot"

$runArgs = @("-p", "zona-p2p-sim", "--bin", "run-sim")
if (-not $DebugBuild) { $runArgs += "--release" }

Push-Location $P2pDir
try {
    & cargo run @runArgs -- @RunArgs
    exit $LASTEXITCODE
} finally {
    Pop-Location
}
