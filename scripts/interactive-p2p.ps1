#requires -Version 5.1
<#
  Интерактивный P2P симулятор / клиент Docker-сети.

  Режимы:
    sandbox  (по умолчанию) — полностью in-process, нет Docker.
    docker   — реальные контейнеры через HTTP admin API.
              Сначала запусти контейнеры: ./scripts/docker-p2p.ps1 -N <N>

  Использование:
    .\scripts\interactive-p2p.ps1                          # sandbox, 8 нод
    .\scripts\interactive-p2p.ps1 -Nodes 12               # sandbox, 12 нод
    .\scripts\interactive-p2p.ps1 -Docker                 # docker, 6 нод
    .\scripts\interactive-p2p.ps1 -Docker -Nodes 8        # docker, 8 нод
    .\scripts\interactive-p2p.ps1 -Docker -Client alice   # docker, имя клиента alice
    .\scripts\interactive-p2p.ps1 -DebugBuild             # без --release
#>
param(
    [int]    $Nodes       = 8,
    [switch] $Docker      = $false,
    [int]    $BasePort    = 17701,
    [string] $Client      = "me",
    [switch] $DebugBuild  = $false
)

$ErrorActionPreference = "Stop"

$RepoRoot = Split-Path -Parent $PSScriptRoot
$P2pDir   = Join-Path $RepoRoot "zona-p2p"
if (-not (Test-Path (Join-Path $P2pDir "Cargo.toml"))) {
    Write-Error "Не найден zona-p2p/Cargo.toml. Запустите из корня репозитория."
}

# ── CARGO_TARGET_DIR (обход кириллических путей) ─────────────────────────────
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

# ── Build sim-interactive ─────────────────────────────────────────────────────
$buildArgs = @("-p", "zona-p2p-sim", "--bin", "sim-interactive")
if (-not $DebugBuild) { $buildArgs += "--release" }

Push-Location $P2pDir
try {
    Write-Host "Building sim-interactive..."
    & cargo build @buildArgs
    if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }

    $profile = if ($DebugBuild) { "debug" } else { "release" }
    $bin     = Join-Path $TargetRoot "$profile\sim-interactive.exe"
    if (-not (Test-Path $bin)) { $bin = Join-Path $TargetRoot "$profile\sim-interactive" }

    Write-Host ""

    if ($Docker) {
        Write-Host "Mode: DOCKER  (nodes: $Nodes, base admin port: $BasePort)"
        Write-Host "Make sure containers are running: ./scripts/docker-p2p.ps1 -N $Nodes"
        Write-Host ""

        $runArgs = @("--docker", "--nodes", $Nodes, "--base-port", $BasePort, "--client", $Client)
        & $bin @runArgs
    } else {
        Write-Host "Mode: SANDBOX  (nodes: $Nodes, client: $Client)"
        Write-Host ""
        & $bin $Nodes --client $Client
    }
    exit $LASTEXITCODE
} finally {
    Pop-Location
}
