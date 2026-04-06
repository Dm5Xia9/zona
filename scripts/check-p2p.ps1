#requires -Version 5.1
<#
  cargo check для workspace zona-p2p с CARGO_TARGET_DIR вне кириллического пути.
  Обходит баги MinGW ld при путях с кириллицей / OneDrive.

  Использование (из корня репозитория):
    .\scripts\check-p2p.ps1
    .\scripts\check-p2p.ps1 --message-format short
#>
param(
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

Push-Location $P2pDir
try {
    & cargo check --workspace @CargoArgs
    exit $LASTEXITCODE
} finally {
    Pop-Location
}
