#!/usr/bin/env pwsh
# docker-p2p.ps1 -- build image, generate docker-compose and start containers.
# Usage: ./scripts/docker-p2p.ps1 [-N <count>] [-Down] [-Logs] [-Rebuild] [-HelloServer] [-ProxyServer] [-DnsServer]
#
# docker compose is run from the zona-p2p/ directory so that relative
# "context: ." in docker-compose.yml resolves correctly (no Cyrillic in path).

param(
    [int]    $N           = 6,
    [int]    $Nodes       = 0,      # alias for -N
    [switch] $Down        = $false,
    [switch] $Logs        = $false,
    [switch] $Rebuild     = $false,
    [switch] $HelloServer = $false,
    [switch] $ProxyServer = $false,
    [switch] $DnsServer   = $false
)
if ($Nodes -gt 0) { $N = $Nodes }

$ErrorActionPreference = "Stop"

$projectDir  = "$PSScriptRoot\..\zona-p2p"
$composeFile = "$projectDir\docker-compose.yml"

# Tear down.
if ($Down) {
    Write-Host "Stopping zona-p2p Docker containers..."
    Push-Location $projectDir
    try { docker compose down --remove-orphans } finally { Pop-Location }
    exit 0
}

# Follow logs.
if ($Logs) {
    Push-Location $projectDir
    try { docker compose logs -f } finally { Pop-Location }
    exit 0
}

# Generate docker-compose.yml.
& "$PSScriptRoot\gen-docker-compose.ps1" -N $N -OutFile $composeFile `
    -HelloServer:$HelloServer -ProxyServer:$ProxyServer -DnsServer:$DnsServer

# Build the node image ONCE (all node containers share the same image).
Write-Host ""
Write-Host "Building zona-p2p Docker image..."
Push-Location $projectDir
try {
    docker build -t zona-p2p .
    if ($LASTEXITCODE -ne 0) {
        Write-Error "docker build failed"
        exit 1
    }
} finally {
    Pop-Location
}

# Build the hello-server image if requested.
if ($HelloServer) {
    Write-Host ""
    Write-Host "Building zona-p2p-hello (hello-server) Docker image..."
    Push-Location $projectDir
    try {
        docker build -t zona-p2p-hello -f Dockerfile.hello-server .
        if ($LASTEXITCODE -ne 0) {
            Write-Error "docker build of hello-server failed"
            exit 1
        }
    } finally {
        Pop-Location
    }
}

# Build the proxy-server image if requested.
if ($ProxyServer) {
    Write-Host ""
    Write-Host "Building zona-p2p-proxy (proxy-server) Docker image..."
    Push-Location $projectDir
    try {
        docker build -t zona-p2p-proxy -f Dockerfile.proxy-server .
        if ($LASTEXITCODE -ne 0) {
            Write-Error "docker build of proxy-server failed"
            exit 1
        }
    } finally {
        Pop-Location
    }
}

# Build the zona-dns image if requested.
if ($DnsServer) {
    Write-Host ""
    Write-Host "Building zona-p2p-dns (zona-dns) Docker image..."
    Push-Location $projectDir
    try {
        docker build -t zona-p2p-dns -f Dockerfile.zona-dns .
        if ($LASTEXITCODE -ne 0) {
            Write-Error "docker build of zona-dns failed"
            exit 1
        }
    } finally {
        Pop-Location
    }
}

Write-Host ""
Write-Host "Stopping any existing containers..."
Push-Location $projectDir
try { docker compose down --remove-orphans 2>$null } catch {} finally { Pop-Location }

$svcNote = if ($ProxyServer -or $HelloServer -or $DnsServer) { " (nodes + extra services)" } else { "" }
Write-Host "Starting stack$svcNote..."
Push-Location $projectDir
try {
    docker compose up -d
    if ($LASTEXITCODE -ne 0) {
        Write-Error "docker compose up failed"
        exit 1
    }
} finally {
    Pop-Location
}

$waitSecs = [Math]::Max(6, 2 + [int]($N * 0.5))
Write-Host ""
Write-Host "Waiting ${waitSecs}s for $N nodes to bootstrap..."
Start-Sleep -Seconds $waitSecs

Write-Host ""
Write-Host "Nodes ready. Admin API URLs (host ports):"
for ($i = 0; $i -lt $N; $i++) {
    $adminPort = 17701 + $i
    Write-Host "  node${i} -> http://localhost:${adminPort}/api/info"
}

$helloServerNodeId = ""
if ($HelloServer) {
    Write-Host "  hello-server -> http://localhost:18800/api/packet"
    Write-Host ""
    Write-Host "Waiting 3s for hello-server to register its NodeId..."
    Start-Sleep -Seconds 3
    # Capture the SERVER_NODE_ID from hello-server logs.
    try {
        Push-Location $projectDir
        try {
            $logs = docker compose logs hello-server 2>&1
        } finally { Pop-Location }
        $match = $logs | Select-String "SERVER_NODE_ID=([0-9a-f]+)"
        if ($match) {
            $helloServerNodeId = $match.Matches[0].Groups[1].Value
            Write-Host "  hello-server NodeId: $helloServerNodeId"
        } else {
            Write-Host "  (Could not detect hello-server NodeId from logs - run: docker compose logs hello-server)"
        }
    } catch {}
}

$proxyNodeId = ""
if ($ProxyServer) {
    Write-Host "  proxy-server -> http://localhost:18801 (P2P HTTP proxy)"
    Write-Host ""
    Write-Host "Waiting 4s for proxy-server to bootstrap..."
    Start-Sleep -Seconds 4
    try {
        Push-Location $projectDir
        try {
            $logs = docker compose logs proxy-server 2>&1
        } finally {
            Pop-Location
        }
        $match = $logs | Select-String "PROXY_NODE_ID=([0-9a-f]{64})"
        if (-not $match) {
            $match = $logs | Select-String "PROXY_NODE_ID=([0-9a-f]+)"
        }
        if ($match) {
            $proxyNodeId = $match.Matches[0].Groups[1].Value
            Write-Host "  proxy-server NodeId: $proxyNodeId"
        } else {
            Write-Host "  (Could not detect proxy NodeId from logs - run: docker compose logs proxy-server)"
        }
    } catch {}
}

$dnsNodeId = ""
if ($DnsServer) {
    Write-Host "  dns-server -> http://localhost:18802/api/dns/lookup?domain=... (zones: zona-p2p/zones.docker.yaml)"
    Write-Host ""
    Write-Host "Waiting 4s for dns-server to bootstrap..."
    Start-Sleep -Seconds 4
    try {
        Push-Location $projectDir
        try {
            $logs = docker compose logs dns-server 2>&1
        } finally {
            Pop-Location
        }
        $match = $logs | Select-String "ZONA_DNS_NODE_ID=([0-9a-f]{64})"
        if (-not $match) {
            $match = $logs | Select-String "ZONA_DNS_NODE_ID=([0-9a-f]+)"
        }
        if ($match) {
            $dnsNodeId = $match.Matches[0].Groups[1].Value
            Write-Host "  dns-server NodeId (for zona-curl --dns-node): $dnsNodeId"
        } else {
            Write-Host "  (Could not detect dns NodeId from logs - run: docker compose logs dns-server)"
        }
    } catch {}
}

Write-Host ""
Write-Host "Run interactive CLI (docker mode):"
$interactiveCmd = "  ./scripts/interactive-p2p.ps1 -Docker -Nodes $N"
if ($HelloServer) {
    $interactiveCmd += " -HelloServer"
    if ($helloServerNodeId) {
        $interactiveCmd += " -HelloServerId $helloServerNodeId"
    }
}
Write-Host $interactiveCmd
if ($ProxyServer -and $proxyNodeId) {
    Write-Host ""
    Write-Host "zona-curl (from host, example):"
    Write-Host "  & `"`$env:LOCALAPPDATA\Zona\cargo-target-p2p\release\zona-curl.exe`" --nodes http://127.0.0.1:17701 --proxy $proxyNodeId -v https://example.com"
}
if ($DnsServer -and $dnsNodeId) {
    Write-Host ""
    Write-Host "zona-curl with DNS (edit zona-p2p/zones.docker.yaml: domain -> proxy hex; restart dns-server if needed):"
    Write-Host "  & `"`$env:LOCALAPPDATA\Zona\cargo-target-p2p\release\zona-curl.exe`" fetch --nodes http://127.0.0.1:17701 --dns-node $dnsNodeId -v https://example.com"
}
Write-Host ""
Write-Host "Follow logs:  ./scripts/docker-p2p.ps1 -Logs"
Write-Host "Stop:         ./scripts/docker-p2p.ps1 -Down"
