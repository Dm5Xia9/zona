#requires -Version 5.1
<#
  Сборка утилиты zona-curl (zona-p2p) с тем же CARGO_TARGET_DIR, что и build-p2p.ps1.
  Обходит баги MinGW ld при путях с кириллицей / OneDrive.

  Использование (из корня репозитория):
    .\scripts\build-zona-curl.ps1              # release
    .\scripts\build-zona-curl.ps1 -DebugBuild  # debug
    .\scripts\build-zona-curl.ps1 -- -v        # доп. флаги cargo (после --)
#>
param(
    [switch]$DebugBuild,
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

$profileDir = if ($DebugBuild) { "debug" } else { "release" }

$buildArgs = @("build", "-p", "zona-curl")
if (-not $DebugBuild) { $buildArgs += "--release" }
$buildArgs += $CargoArgs

Push-Location $P2pDir
try {
    & cargo @buildArgs
    if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
} finally {
    Pop-Location
}

$outDir = Join-Path $TargetRoot $profileDir
$exeName = if ($isWin) { "zona-curl.exe" } else { "zona-curl" }
$binPath = Join-Path $outDir $exeName
if (-not (Test-Path -LiteralPath $binPath)) {
    Write-Error "Не найден бинарник: $binPath"
}

Write-Host ""
Write-Host "OK: $binPath"
Write-Host "Example: & `"$binPath`" --help"