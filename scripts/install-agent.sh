#!/usr/bin/env bash
# LocalForge agent installer — Linux (Debian/Ubuntu/RHEL/Fedora/Alpine/Arch).
#
# Usage (run as root or via sudo):
#   curl -sSL https://github.com/fabri2000779/localforge/releases/latest/download/install-agent.sh | sudo bash
#
# The script:
#   1. Detects the OS + architecture
#   2. Installs Docker if it isn't already present
#   3. Downloads the matching localforge-agent binary
#   4. Creates a system user, data dir, and 0600 config file
#   5. Bootstraps a fresh token + self-signed TLS cert (via `agent install`)
#   6. Registers + starts the systemd service
#   7. Prints the pairing block (URL/token/fingerprint) for the desktop
#
# Optional environment overrides:
#   LOCALFORGE_AGENT_VERSION    — release tag (default: latest)
#   LOCALFORGE_AGENT_BIND       — bind interface (default: 0.0.0.0)
#   LOCALFORGE_AGENT_PORT       — TCP port (default: 7878)
#   LOCALFORGE_AGENT_DATA_ROOT  — data dir (default: /var/lib/localforge)
#   LOCALFORGE_AGENT_DOMAIN     — DNS name to surface in pairing output
#                                 (default: auto-detect public IPv4)
#   LOCALFORGE_AGENT_LABEL      — friendly node label printed in summary

set -euo pipefail

REPO="fabri2000779/localforge"
INSTALL_DIR="/usr/local/bin"
CONFIG_DIR="/etc/localforge"
CONFIG_PATH="${CONFIG_DIR}/agent.toml"
DATA_ROOT="${LOCALFORGE_AGENT_DATA_ROOT:-/var/lib/localforge}"
BIND="${LOCALFORGE_AGENT_BIND:-0.0.0.0}"
PORT="${LOCALFORGE_AGENT_PORT:-7878}"
SERVICE_USER="localforge"
SERVICE_PATH="/etc/systemd/system/localforge-agent.service"
LABEL="${LOCALFORGE_AGENT_LABEL:-$(hostname -s 2>/dev/null || echo localforge-node)}"

# ---------------------------------------------------------------------------
# Helpers
# ---------------------------------------------------------------------------

log()  { printf '\033[1;36m▸\033[0m %s\n' "$*"; }
ok()   { printf '\033[1;32m✓\033[0m %s\n' "$*"; }
warn() { printf '\033[1;33m!\033[0m %s\n' "$*" >&2; }
die()  { printf '\033[1;31m✗\033[0m %s\n' "$*" >&2; exit 1; }

require_root() {
  if [[ $EUID -ne 0 ]]; then
    die "This installer needs root. Re-run with: sudo bash $0  (or pipe via | sudo bash)."
  fi
}

# ---------------------------------------------------------------------------
# Step 1 — environment detection
# ---------------------------------------------------------------------------

detect_arch() {
  case "$(uname -m)" in
    x86_64|amd64)   ARCH_TARGET="x86_64-unknown-linux-gnu" ;;
    aarch64|arm64)  ARCH_TARGET="aarch64-unknown-linux-gnu" ;;
    *) die "Unsupported architecture: $(uname -m). LocalForge agent supports x86_64 and aarch64." ;;
  esac
  ok "Architecture: ${ARCH_TARGET}"
}

detect_distro() {
  if [[ -r /etc/os-release ]]; then
    # shellcheck disable=SC1091
    . /etc/os-release
    DISTRO_ID="${ID:-unknown}"
    DISTRO_LIKE="${ID_LIKE:-}"
  else
    DISTRO_ID="unknown"
    DISTRO_LIKE=""
  fi

  case "$DISTRO_ID" in
    ubuntu|debian|raspbian) PKG_FAMILY="debian" ;;
    rhel|centos|rocky|almalinux|ol|amzn|fedora) PKG_FAMILY="rhel" ;;
    alpine)   PKG_FAMILY="alpine" ;;
    arch|manjaro|cachyos|endeavouros) PKG_FAMILY="arch" ;;
    opensuse*|sles) PKG_FAMILY="suse" ;;
    *)
      # Fall back to ID_LIKE.
      case "$DISTRO_LIKE" in
        *debian*) PKG_FAMILY="debian" ;;
        *rhel*|*fedora*) PKG_FAMILY="rhel" ;;
        *arch*) PKG_FAMILY="arch" ;;
        *suse*) PKG_FAMILY="suse" ;;
        *) PKG_FAMILY="unknown" ;;
      esac
      ;;
  esac
  ok "Distribution: ${DISTRO_ID} (family: ${PKG_FAMILY})"
}

# ---------------------------------------------------------------------------
# Step 2 — Docker
# ---------------------------------------------------------------------------

ensure_docker() {
  if command -v docker >/dev/null 2>&1 && docker info >/dev/null 2>&1; then
    ok "Docker already installed and reachable."
    return
  fi

  if command -v docker >/dev/null 2>&1; then
    warn "Docker binary present but daemon is not responding. Attempting to start it..."
    systemctl enable --now docker 2>/dev/null || service docker start 2>/dev/null || true
    if docker info >/dev/null 2>&1; then
      ok "Docker started."
      return
    fi
  fi

  log "Installing Docker (this may take a couple of minutes)..."
  case "$PKG_FAMILY" in
    debian|rhel|suse)
      # Docker's official one-shot installer covers all these families.
      if ! command -v curl >/dev/null 2>&1; then
        install_curl
      fi
      curl -fsSL https://get.docker.com | sh
      ;;
    alpine)
      apk add --no-cache docker docker-cli
      rc-update add docker boot
      service docker start
      ;;
    arch)
      pacman -Sy --noconfirm docker
      systemctl enable --now docker
      ;;
    *)
      die "Cannot auto-install Docker on this distribution (${DISTRO_ID}). Install Docker manually and re-run this script."
      ;;
  esac

  # Make sure daemon is up.
  systemctl enable --now docker 2>/dev/null || true
  if ! docker info >/dev/null 2>&1; then
    die "Docker installed but daemon is not responding. Try: systemctl status docker"
  fi
  ok "Docker installed and running."
}

install_curl() {
  case "$PKG_FAMILY" in
    debian) apt-get update -qq && apt-get install -y curl ca-certificates ;;
    rhel)   (dnf -y install curl ca-certificates 2>/dev/null) || yum -y install curl ca-certificates ;;
    alpine) apk add --no-cache curl ca-certificates ;;
    arch)   pacman -Sy --noconfirm curl ca-certificates ;;
    suse)   zypper --non-interactive install curl ca-certificates ;;
    *) die "curl missing and unknown package manager." ;;
  esac
}

# ---------------------------------------------------------------------------
# Step 3 — release binary
# ---------------------------------------------------------------------------

resolve_version() {
  VERSION="${LOCALFORGE_AGENT_VERSION:-}"
  if [[ -z "$VERSION" ]]; then
    VERSION=$(curl -fsSL "https://api.github.com/repos/${REPO}/releases/latest" \
      | sed -nE 's/.*"tag_name":\s*"([^"]+)".*/\1/p' | head -n1)
  fi
  [[ -n "$VERSION" ]] || die "Could not resolve LocalForge agent version."
  ok "Release: ${VERSION}"
}

download_binary() {
  local url="https://github.com/${REPO}/releases/download/${VERSION}/localforge-agent-${ARCH_TARGET}"
  log "Downloading agent binary..."
  curl -fL --retry 3 -o "${INSTALL_DIR}/localforge-agent.tmp" "$url"
  chmod +x "${INSTALL_DIR}/localforge-agent.tmp"
  mv -f "${INSTALL_DIR}/localforge-agent.tmp" "${INSTALL_DIR}/localforge-agent"
  ok "Installed to ${INSTALL_DIR}/localforge-agent"
}

# ---------------------------------------------------------------------------
# Step 4 — service user + data dir
# ---------------------------------------------------------------------------

ensure_service_user() {
  if ! id -u "$SERVICE_USER" >/dev/null 2>&1; then
    if [[ "$PKG_FAMILY" == "alpine" ]]; then
      adduser -S -h "$DATA_ROOT" -s /sbin/nologin "$SERVICE_USER"
    else
      useradd --system --home-dir "$DATA_ROOT" --shell /usr/sbin/nologin "$SERVICE_USER" \
        || useradd --system --home-dir "$DATA_ROOT" "$SERVICE_USER"
    fi
  fi
  # Add to docker group so the agent can hit the daemon socket without root.
  usermod -aG docker "$SERVICE_USER" 2>/dev/null || addgroup "$SERVICE_USER" docker 2>/dev/null || true
  mkdir -p "$DATA_ROOT" "$CONFIG_DIR"
  chown -R "${SERVICE_USER}:${SERVICE_USER}" "$DATA_ROOT"
  ok "System user: ${SERVICE_USER}"
}

# ---------------------------------------------------------------------------
# Step 5 — config + cert
# ---------------------------------------------------------------------------

ensure_config() {
  if [[ -f "$CONFIG_PATH" ]]; then
    warn "Existing config at ${CONFIG_PATH}. Keeping it as-is (delete it first to regenerate)."
    return
  fi
  log "Generating bearer token + self-signed TLS cert..."
  "${INSTALL_DIR}/localforge-agent" install \
    --config-path "$CONFIG_PATH" \
    --data-root "$DATA_ROOT" \
    --bind "$BIND" \
    --port "$PORT" > /tmp/localforge-pairing.txt
  chown "${SERVICE_USER}:${SERVICE_USER}" "$CONFIG_PATH"
  chmod 600 "$CONFIG_PATH"
  ok "Config written to ${CONFIG_PATH} (0600)."
}

# ---------------------------------------------------------------------------
# Step 6 — systemd unit
# ---------------------------------------------------------------------------

install_systemd_unit() {
  if ! command -v systemctl >/dev/null 2>&1; then
    warn "systemctl not found — starting agent in the foreground is your responsibility."
    return
  fi
  cat > "$SERVICE_PATH" <<UNIT
[Unit]
Description=LocalForge remote agent
After=network-online.target docker.service
Wants=network-online.target
Requires=docker.service

[Service]
Type=simple
User=${SERVICE_USER}
Group=${SERVICE_USER}
ExecStart=${INSTALL_DIR}/localforge-agent --config ${CONFIG_PATH} serve
Restart=on-failure
RestartSec=5s
ProtectSystem=full
ProtectHome=true
NoNewPrivileges=true
ReadWritePaths=${DATA_ROOT}

[Install]
WantedBy=multi-user.target
UNIT

  systemctl daemon-reload
  systemctl enable --now localforge-agent
  ok "Systemd service registered and started."
}

# ---------------------------------------------------------------------------
# Step 7 — pairing summary
# ---------------------------------------------------------------------------

detect_public_address() {
  if [[ -n "${LOCALFORGE_AGENT_DOMAIN:-}" ]]; then
    PUBLIC_HOST="$LOCALFORGE_AGENT_DOMAIN"
    return
  fi
  # Try a few public-IP services with short timeouts.
  for service in \
    "https://api.ipify.org" \
    "https://ifconfig.me/ip" \
    "https://icanhazip.com" \
    "https://checkip.amazonaws.com"
  do
    PUBLIC_HOST=$(curl -fsSL --max-time 3 "$service" 2>/dev/null | tr -d '[:space:]' || true)
    if [[ -n "$PUBLIC_HOST" ]]; then return; fi
  done
  PUBLIC_HOST="$(hostname -I 2>/dev/null | awk '{print $1}')"
  [[ -n "$PUBLIC_HOST" ]] || PUBLIC_HOST="<server-ip>"
}

print_summary() {
  detect_public_address

  # Extract the pairing block emitted by `agent install`. Each field lives on
  # a line like "  Token:    lf_agent_..." — we just regex them out so we
  # can rewrite the URL with the detected public host.
  local token fingerprint
  token=$(grep -oE 'lf_agent_[a-f0-9]+' /tmp/localforge-pairing.txt 2>/dev/null | head -n1 || true)
  fingerprint=$(grep -oE 'SHA256:[A-F0-9:]+' /tmp/localforge-pairing.txt 2>/dev/null | head -n1 || true)
  rm -f /tmp/localforge-pairing.txt

  cat <<SUMMARY

────────────────────────────────────────────────────────────────────────
  ✓ LocalForge agent installed and running on this host.
────────────────────────────────────────────────────────────────────────

  Paste these three values into the desktop's "Add Node" form:

    Label:        ${LABEL}
    URL:          https://${PUBLIC_HOST}:${PORT}
    Token:        ${token:-<see "${CONFIG_PATH}">}
    Fingerprint:  ${fingerprint:-<run: localforge-agent install --help>}

  Status: systemctl status localforge-agent
  Logs:   journalctl -u localforge-agent -f

  If you have a domain pointing at this server, you can re-run with:
    LOCALFORGE_AGENT_DOMAIN=node.example.com bash $0

SUMMARY
}

# ---------------------------------------------------------------------------
# main
# ---------------------------------------------------------------------------

require_root
detect_arch
detect_distro
resolve_version
ensure_docker
download_binary
ensure_service_user
ensure_config
install_systemd_unit
print_summary
