#requires -Version 5.1
<#
  Запуск тестов Rust-крейта zona с тем же окружением, что и build-zona.ps1:
  CARGO_TARGET_DIR вне пути проекта (только ASCII), при необходимости — MinGW в PATH.

  Использование (из корня репозитория):
    .\scripts\test-zona.ps1
    .\scripts\test-zona.ps1 -- --nocapture
#>
param(
    [Parameter(ValueFromRemainingArguments = $true)]
    [string[]]$CargoArgs
)

$ErrorActionPreference = "Stop"

$RepoRoot = Split-Path -Parent $PSScriptRoot
$ZonaDir = Join-Path $RepoRoot "zona"
if (-not (Test-Path (Join-Path $ZonaDir "Cargo.toml"))) {
    Write-Error "Не найден zona/Cargo.toml. Запустите: .\scripts\test-zona.ps1 из корня репозитория."
}

$isWin = [System.Runtime.InteropServices.RuntimeInformation]::IsOSPlatform(
    [System.Runtime.InteropServices.OSPlatform]::Windows)

if ($isWin) {
    $localApp = $env:LOCALAPPDATA
    if ([string]::IsNullOrEmpty($localApp)) {
        Write-Error "LOCALAPPDATA не задан."
    }
    $TargetRoot = Join-Path $localApp "Zona\cargo-target"
    $msysMingwBin = "C:\msys64\mingw64\bin"
    if (Test-Path (Join-Path $msysMingwBin "dlltool.exe")) {
        $env:Path = "${msysMingwBin};${env:Path}"
    }
} else {
    $homeDir = $env:HOME
    if ([string]::IsNullOrEmpty($homeDir)) {
        Write-Error "HOME не задан."
    }
    $TargetRoot = Join-Path $homeDir ".cache/zona-cargo-target"
}

New-Item -ItemType Directory -Force $TargetRoot | Out-Null
$env:CARGO_TARGET_DIR = $TargetRoot
Write-Host "CARGO_TARGET_DIR=$TargetRoot"

Push-Location $ZonaDir
try {
    & cargo test @CargoArgs
    exit $LASTEXITCODE
} finally {
    Pop-Location
}
