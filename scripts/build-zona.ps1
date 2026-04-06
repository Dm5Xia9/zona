#requires -Version 5.1
<#
  Сборка Rust-крейта zona с CARGO_TARGET_DIR вне пути проекта (только ASCII).
  Обходит баги MinGW ld при путях с кириллицей / OneDrive (например ...\Документы\...).
  После успеха копирует бинарник в zona/target/{release|debug}/ для Zona.Api и csproj CopyToOutputDirectory.

  Использование (из корня репозитория):
    .\scripts\build-zona.ps1
    .\scripts\build-zona.ps1 -DebugBuild
    # Тесты с тем же окружением: .\scripts\test-zona.ps1
    # Доп. флаги cargo: через $CargoArgs или вручную задайте CARGO_TARGET_DIR и выполните cargo в каталоге zona/.
#>
param(
    [switch]$DebugBuild,
    [Parameter(ValueFromRemainingArguments = $true)]
    [string[]]$CargoArgs
)

$ErrorActionPreference = "Stop"

$RepoRoot = Split-Path -Parent $PSScriptRoot
$ZonaDir = Join-Path $RepoRoot "zona"
if (-not (Test-Path (Join-Path $ZonaDir "Cargo.toml"))) {
    Write-Error "Не найден zona/Cargo.toml. Запустите: .\scripts\build-zona.ps1 из корня репозитория."
}

$isWin = [System.Runtime.InteropServices.RuntimeInformation]::IsOSPlatform(
    [System.Runtime.InteropServices.OSPlatform]::Windows)

if ($isWin) {
    $localApp = $env:LOCALAPPDATA
    if ([string]::IsNullOrEmpty($localApp)) {
        Write-Error "LOCALAPPDATA не задан."
    }
    $TargetRoot = Join-Path $localApp "Zona\cargo-target"
    # Rust windows-gnu: windows-sys и др. требуют dlltool/binutils из полного MinGW (winget: MSYS2 + mingw-w64-x86_64-toolchain).
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

$profileDir = if ($DebugBuild) { "debug" } else { "release" }

Push-Location $ZonaDir
try {
    if ($DebugBuild) {
        & cargo build @CargoArgs
    } else {
        & cargo build --release @CargoArgs
    }
    if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
} finally {
    Pop-Location
}

$outDir = Join-Path $TargetRoot $profileDir
$built = $null
foreach ($name in @("zona.exe", "zona")) {
    $p = Join-Path $outDir $name
    if (Test-Path -LiteralPath $p) {
        $built = $p
        break
    }
}

if (-not $built) {
    Write-Error "Не найден бинарник в $outDir (ожидались zona.exe или zona)."
}

$destDir = Join-Path $ZonaDir "target\$profileDir"
New-Item -ItemType Directory -Force $destDir | Out-Null
$destPath = Join-Path $destDir (Split-Path -Leaf $built)
Copy-Item -LiteralPath $built -Destination $destPath -Force
Write-Host "Скопировано: $destPath"
