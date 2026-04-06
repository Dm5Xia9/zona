#requires -Version 5.1
<#
  Запуск тестов workspace zona-p2p с CARGO_TARGET_DIR вне кириллического пути.

  Использование (из корня репозитория):
    .\scripts\test-p2p.ps1
    .\scripts\test-p2p.ps1 -- --nocapture
    .\scripts\test-p2p.ps1 -Crate zona-p2p-overlay
    .\scripts\test-p2p.ps1 -Crate zona-p2p-sim -- --nocapture
    .\scripts\test-p2p.ps1 -Filter routing   # запустить тесты по имени
#>
param(
    [string]$Crate = "",
    [Parameter(ValueFromRemainingArguments = $true)]
    [string[]]$CargoArgs
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

$testArgs = @()
if ($Crate -ne "") { $testArgs += "-p"; $testArgs += $Crate }
else { $testArgs += "--workspace" }

Push-Location $P2pDir
try {
    & cargo test @testArgs @CargoArgs
    exit $LASTEXITCODE
} finally {
    Pop-Location
}
