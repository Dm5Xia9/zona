#!/usr/bin/env bash
# Bootstrap: install Docker if missing, download latest zona-p2p node release,
# write Dockerfile + docker-compose in a dedicated folder, start stack in background.
#
# Usage:
#   From repo clone: ./bootstrap-zona-node-stack.sh  (releases: origin if GitHub, else default below)
#   One-liner (no clone): see README (curl | bash)
#
# Env:
#   ZONA_DEPLOY_DIR   — output directory (default: <work-dir>/zona-node-stack)
#   ZONA_GITHUB_REPO  — optional override: "owner/name" for GitHub releases (default: Dm5Xia9/zona)
#   GITHUB_TOKEN      — optional, raises API rate limit for private repos

set -euo pipefail

# When run as `curl ... | bash`, BASH_SOURCE is "-" or a /dev/fd path — use current directory.
_script="${BASH_SOURCE[0]:-}"
if [[ -z "$_script" || "$_script" == "-" || "$_script" == */dev/fd/* || "$_script" == */proc/self/fd/* ]]; then
  WORK_ROOT="$(pwd)"
else
  WORK_ROOT="$(cd "$(dirname "$_script")" && pwd)"
fi
DEPLOY_DIR="${ZONA_DEPLOY_DIR:-$WORK_ROOT/zona-node-stack}"
ADMIN_HOST_PORT=17701
# Upstream releases for prebuilt binaries (override with ZONA_GITHUB_REPO).
DEFAULT_ZONA_GITHUB_REPO="Dm5Xia9/zona"

ASSET_NAME="zona-p2p-linux-x64.tar.gz"
# Release ships Linux x64 binary; use amd64 platform on ARM hosts so the container can run it.
HOST_ARCH="$(uname -m)"
COMPOSE_PLATFORM=""
if [[ "$HOST_ARCH" == "aarch64" || "$HOST_ARCH" == "arm64" ]]; then
  COMPOSE_PLATFORM="linux/amd64"
fi

log() { printf '%s\n' "$*"; }
die() { printf 'error: %s\n' "$*" >&2; exit 1; }

command_exists() { command -v "$1" >/dev/null 2>&1; }

ensure_curl() {
  command_exists curl || die "curl is required"
}

# 64 hex chars = 32 bytes for ZONA_NODE_SEED (Ed25519 key material).
random_node_seed_hex() {
  if command_exists openssl; then
    openssl rand -hex 32
    return 0
  fi
  if command_exists python3; then
    python3 -c "import secrets; print(secrets.token_hex(32))"
    return 0
  fi
  die "install openssl or python3 to generate ZONA_NODE_SEED"
}

resolve_github_repo() {
  if [[ -n "${ZONA_GITHUB_REPO:-}" ]]; then
    printf '%s\n' "$ZONA_GITHUB_REPO"
    return 0
  fi
  local url
  url="$(git -C "$WORK_ROOT" config --get remote.origin.url 2>/dev/null || true)"
  if [[ -n "$url" ]] && [[ "$url" =~ github\.com[:/]([^/]+)/([^/.]+)(\.git)?$ ]]; then
    printf '%s/%s\n' "${BASH_REMATCH[1]}" "${BASH_REMATCH[2]}"
    return 0
  fi
  printf '%s\n' "$DEFAULT_ZONA_GITHUB_REPO"
}

ensure_docker() {
  if command_exists docker; then
    return 0
  fi
  local os
  os="$(uname -s)"
  if [[ "$os" != "Linux" ]]; then
    die "Docker is not installed. Install Docker Desktop for $os, then re-run this script."
  fi
  log "Docker not found; installing via https://get.docker.com (needs sudo)..."
  ensure_curl
  log "Downloading Docker installer script..."
  if [[ "${EUID:-}" -ne 0 ]]; then
    curl -fL --progress-bar https://get.docker.com | sudo sh
  else
    curl -fL --progress-bar https://get.docker.com | sh
  fi
  command_exists docker || die "Docker install finished but docker is still not in PATH"
}

compose_cmd() {
  if docker info >/dev/null 2>&1; then
    if docker compose version >/dev/null 2>&1; then
      docker compose "$@"
    elif command_exists docker-compose; then
      docker-compose "$@"
    else
      die "docker compose plugin not found; install Docker Compose v2"
    fi
  elif sudo -n docker info >/dev/null 2>&1; then
    if sudo -n docker compose version >/dev/null 2>&1; then
      sudo -n docker compose "$@"
    elif command_exists docker-compose; then
      sudo -n docker-compose "$@"
    else
      die "docker compose plugin not found; install Docker Compose v2"
    fi
  else
    die "cannot run docker (permission denied?). Add user to group docker or use sudo once configured for passwordless docker, then retry."
  fi
}

asset_download_url() {
  local json="$1" name="$2" url
  if command_exists jq; then
    url="$(jq -r --arg n "$name" '.assets[] | select(.name == $n) | .browser_download_url' <<<"$json")"
    if [[ -n "$url" && "$url" != "null" ]]; then
      printf '%s\n' "$url"
      return 0
    fi
  fi
  if command_exists python3; then
    url="$(python3 -c "
import json, sys
name = sys.argv[1]
data = json.loads(sys.stdin.read())
for a in data.get('assets', []):
    if a.get('name') == name:
        u = a.get('browser_download_url') or ''
        if u:
            print(u)
        break
" "$name" <<<"$json")"
    if [[ -n "$url" ]]; then
      printf '%s\n' "$url"
      return 0
    fi
  fi
  return 1
}

download_latest_node() {
  local repo api asset_url
  repo="$(resolve_github_repo)"
  api="https://api.github.com/repos/${repo}/releases/latest"
  ensure_curl
  command_exists jq || command_exists python3 || die "install jq or python3 to parse GitHub release JSON"
  local hdr=()
  [[ -z "${GITHUB_TOKEN:-}" ]] || hdr=(-H "Authorization: Bearer ${GITHUB_TOKEN}")

  log "Fetching latest release metadata from ${repo}..."
  local json
  json="$(curl -fL --progress-bar "${hdr[@]}" "$api")" || die "failed to fetch $api"

  asset_url="$(asset_download_url "$json" "$ASSET_NAME")" || true
  [[ -n "${asset_url:-}" ]] || die "asset ${ASSET_NAME} not found in latest release (publish release workflow first?)"

  mkdir -p "$DEPLOY_DIR"
  local tgz="$DEPLOY_DIR/$ASSET_NAME"
  log "Downloading ${ASSET_NAME}..."
  curl -fL --progress-bar -o "$tgz" "${hdr[@]}" "$asset_url"

  tar -xzf "$tgz" -C "$DEPLOY_DIR"
  local bin="$DEPLOY_DIR/linux-x64/zona-p2p"
  [[ -f "$bin" ]] || bin="$DEPLOY_DIR/zona-p2p"
  [[ -f "$bin" ]] || die "extracted archive but zona-p2p not found under $DEPLOY_DIR"
  chmod +x "$bin"
  # Flatten: Dockerfile expects ./zona-p2p next to it
  if [[ "$bin" != "$DEPLOY_DIR/zona-p2p" ]]; then
    mv -f "$bin" "$DEPLOY_DIR/zona-p2p"
    rmdir "$DEPLOY_DIR/linux-x64" 2>/dev/null || true
  fi
  rm -f "$tgz"
}

write_dockerfile() {
  cat >"$DEPLOY_DIR/Dockerfile" <<'EOF'
FROM debian:bookworm-slim

RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates \
 && rm -rf /var/lib/apt/lists/*

COPY zona-p2p /usr/local/bin/zona-p2p
RUN chmod +x /usr/local/bin/zona-p2p

EXPOSE 7701
ENTRYPOINT ["zona-p2p"]
EOF
}

write_compose() {
  local seed_hex="$1"
  local out="$DEPLOY_DIR/docker-compose.yml"
  {
    echo "# Generated by bootstrap-zona-node-stack.sh — single zona-p2p node, prebuilt binary from GitHub release."
    echo "networks:"
    echo "  zona-net:"
    echo "    driver: bridge"
    echo ""
    echo "services:"
    echo "  node:"
    echo "    build:"
    echo "      context: ."
    echo "      dockerfile: Dockerfile"
    echo "    image: zona-p2p-prebuilt:local"
    if [[ -n "$COMPOSE_PLATFORM" ]]; then
      echo "    platform: ${COMPOSE_PLATFORM}"
    fi
    echo "    networks:"
    echo "      - zona-net"
    echo "    ports:"
    echo "      - \"${ADMIN_HOST_PORT}:7701\""
    echo "    environment:"
    echo "      ZONA_NODE_SEED: '${seed_hex}'"
    echo "      ZONA_ADMIN_URL: 'http://node:7701'"
    echo "      ZONA_ADMIN_PORT: '7701'"
    echo "      RUST_LOG: 'info'"
    echo "    restart: unless-stopped"
  } >"$out"
  log "Wrote $out"
}

main() {
  ensure_curl
  ensure_docker
  download_latest_node
  write_dockerfile
  write_compose "$(random_node_seed_hex)"

  if [[ -n "$COMPOSE_PLATFORM" ]]; then
    log "Host is ${HOST_ARCH}: using compose platform ${COMPOSE_PLATFORM} (Linux/x64 node binary inside image)."
  fi

  log "Building image and starting container in background..."
  (cd "$DEPLOY_DIR" && compose_cmd up -d --build)

  log ""
  log "Stack directory: $DEPLOY_DIR"
  log "Admin API: http://localhost:${ADMIN_HOST_PORT}/api/info"
  log ""
  log "Logs: (cd \"$DEPLOY_DIR\" && docker compose logs -f)"
  log "Stop: (cd \"$DEPLOY_DIR\" && docker compose down)"
}

main "$@"
