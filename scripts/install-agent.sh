#!/usr/bin/env bash
# LocalForge agent installer — Linux x86_64 / arm64.
#
# Usage (run as root or via sudo):
#   curl -sSL https://github.com/fabri2000779/localforge/releases/latest/download/install-agent.sh | sudo bash
#
# Optional environment overrides:
#   LOCALFORGE_AGENT_VERSION   — release tag to install (default: latest)
#   LOCALFORGE_AGENT_BIND      — interface to bind (default: 0.0.0.0)
#   LOCALFORGE_AGENT_PORT      — TCP port (default: 7878)
#   LOCALFORGE_AGENT_DATA_ROOT — data directory (default: /var/lib/localforge)

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

if [[ $EUID -ne 0 ]]; then
  echo "This installer needs root (use sudo)." >&2
  exit 1
fi

case "$(uname -m)" in
  x86_64|amd64) ARCH="x86_64-unknown-linux-gnu" ;;
  aarch64|arm64) ARCH="aarch64-unknown-linux-gnu" ;;
  *) echo "Unsupported architecture: $(uname -m)" >&2; exit 1 ;;
esac

# Resolve release tag.
VERSION="${LOCALFORGE_AGENT_VERSION:-}"
if [[ -z "$VERSION" ]]; then
  VERSION=$(curl -fsSL "https://api.github.com/repos/${REPO}/releases/latest" \
    | sed -nE 's/.*"tag_name":\s*"([^"]+)".*/\1/p' | head -n1)
fi
if [[ -z "$VERSION" ]]; then
  echo "Could not determine the latest LocalForge agent version." >&2
  exit 1
fi

BIN_URL="https://github.com/${REPO}/releases/download/${VERSION}/localforge-agent-${ARCH}"

echo "Installing localforge-agent ${VERSION} (${ARCH})..."
echo "  -> ${INSTALL_DIR}/localforge-agent"

# Docker dependency check.
if ! command -v docker >/dev/null 2>&1; then
  echo
  echo "Docker was not found on this host. Install Docker first, e.g.:"
  echo "  curl -fsSL https://get.docker.com | sh"
  echo "Then re-run this installer."
  exit 1
fi

curl -fL --retry 3 -o "${INSTALL_DIR}/localforge-agent" "$BIN_URL"
chmod +x "${INSTALL_DIR}/localforge-agent"

# Create a dedicated service user (added to the docker group so the agent
# can talk to the daemon socket without root).
if ! id -u "$SERVICE_USER" >/dev/null 2>&1; then
  useradd --system --home-dir "$DATA_ROOT" --shell /usr/sbin/nologin "$SERVICE_USER"
fi
usermod -aG docker "$SERVICE_USER" || true

mkdir -p "$CONFIG_DIR" "$DATA_ROOT"
chown -R "${SERVICE_USER}:${SERVICE_USER}" "$DATA_ROOT"

# Generate config (token + self-signed cert) if it doesn't exist.
if [[ ! -f "$CONFIG_PATH" ]]; then
  "${INSTALL_DIR}/localforge-agent" install \
    --config-path "$CONFIG_PATH" \
    --data-root "$DATA_ROOT" \
    --bind "$BIND" \
    --port "$PORT"
  chown "${SERVICE_USER}:${SERVICE_USER}" "$CONFIG_PATH"
  chmod 600 "$CONFIG_PATH"
else
  echo "Existing config at ${CONFIG_PATH} — keeping it as-is."
fi

# Systemd unit.
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

echo
echo "localforge-agent installed and running."
echo "Status: systemctl status localforge-agent"
echo "Logs:   journalctl -u localforge-agent -f"
