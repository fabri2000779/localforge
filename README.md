<div align="center">

# 🔨 LocalForge

**Self-hosted game server panel — runs on your laptop, scales to your VPS.**

A native desktop app for running game servers without a web panel.
Pair remote agents over HTTPS and manage local + remote game servers from the same UI.

### **[⬇️ Download for your OS at localforge.gg](https://localforge.gg)** &nbsp;·&nbsp; [Releases](https://github.com/fabri2000779/localforge/releases/latest)

[![Website](https://img.shields.io/badge/website-localforge.gg-3b82f6)](https://localforge.gg)
[![Release](https://img.shields.io/github/v/release/fabri2000779/localforge?include_prereleases&sort=semver)](https://github.com/fabri2000779/localforge/releases)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)
[![Built with Tauri](https://img.shields.io/badge/Tauri-2.x-24C8DB?logo=tauri)](https://tauri.app/)
[![Rust](https://img.shields.io/badge/Rust-1.95-DEA584?logo=rust)](https://www.rust-lang.org/)
[![React](https://img.shields.io/badge/React-19-61DAFB?logo=react)](https://react.dev/)

</div>

---

## ✨ Features

- 🖱️ **One-click servers** — pick a game, click create. No terminal, no Docker knowledge.
- 🌐 **Multi-node** — manage local Docker **and** any number of remote VPS agents from one app
- 🔒 **Built-in security** — bearer tokens, self-signed TLS with fingerprint pinning, or BYO Let's Encrypt cert
- 📦 **Battle-tested Docker images** — uses the same `parkervcp/yolks` and `parkervcp/games` registries the wider game-server-hosting community relies on
- 📺 **Live console & logs** — streamed over WebSocket from local or remote nodes
- 📁 **File manager** — browse, edit, **upload/download** with chunked streaming (multi-GB worlds work fine)
- 📊 **Per-node host stats** — CPU/RAM/disk gauges for every node, refreshed every 5 s
- 🔄 **Install scripts** — OAuth URLs that show up in the install pipeline auto-open in your local browser, even when the install runs on a remote VPS
- 🎮 **10+ games out of the box** — Minecraft Java/Bedrock, Hytale, Valheim, Terraria, Factorio, 7 Days to Die, Rust, Palworld, Sons of the Forest, Project Zomboid… plus custom-game editor

---

## 🎮 Supported games

| Game | Backing image |
|------|--------------|
| Minecraft Java | `ghcr.io/parkervcp/yolks:java_21` |
| Minecraft Bedrock | `ghcr.io/parkervcp/yolks:debian` |
| Hytale | `ghcr.io/parkervcp/yolks:java_25` |
| Terraria | `ghcr.io/parkervcp/yolks:debian` |
| Rust | `ghcr.io/parkervcp/games:rust` |
| Palworld | `ghcr.io/parkervcp/yolks:debian` |
| Satisfactory | `ghcr.io/parkervcp/yolks:debian` |
| Project Zomboid | `ghcr.io/parkervcp/yolks:debian` |
| Sons of the Forest | `ghcr.io/parkervcp/yolks:wine_latest` |
| StarRupture | `ghcr.io/parkervcp/yolks:wine_latest` |

Add your own via **Settings → Custom games** using a JSON template.

---

## 🚀 Quick start

### 1 · Install the desktop app

Easiest: **[localforge.gg](https://localforge.gg)** — the download button detects your OS and links straight to the right installer.

Or pick a build by hand from [Releases](https://github.com/fabri2000779/localforge/releases/latest):

| OS | Asset |
|----|-------|
| Windows | `LocalForge_x.y.z_x64-setup.exe` (NSIS installer) |
| macOS | `LocalForge_x.y.z_universal.dmg` (Apple Silicon + Intel) — signed & notarized |
| Linux (Debian/Ubuntu) | `LocalForge_x.y.z_amd64.deb` |
| Linux (RHEL/Fedora) | `LocalForge-x.y.z-1.x86_64.rpm` |
| Linux (any) | `LocalForge_x.y.z_amd64.AppImage` |

**Requires** [Docker Desktop](https://www.docker.com/products/docker-desktop/) (Windows/macOS) or Docker Engine (Linux). The app shows a clear "install Docker" screen if it's missing.

### 2 · Add a remote node (optional)

For local-only use, skip this step — the **Local** node is already there.

On your VPS, run **one** of these as root:

**Linux:**
```bash
curl -sSL https://github.com/fabri2000779/localforge/releases/latest/download/install-agent.sh \
  | sudo bash
```

**Windows / Windows Server** (elevated PowerShell):
```powershell
iex "& { $(irm https://github.com/fabri2000779/localforge/releases/latest/download/install-agent.ps1) }"
```

Both scripts:
- Detect distro / architecture
- Install Docker if missing (Linux only; Windows points you to Docker Desktop)
- Download the matching `localforge-agent` binary
- Generate a bearer token + self-signed TLS cert
- Register a systemd service (Linux) or scheduled task (Windows) that auto-starts at boot
- Print the pairing data:
  ```
  URL:          https://your-vps:7878
  Token:        lf_agent_…
  Fingerprint:  SHA256:AB:CD:…
  ```

In the desktop app: **Nodes → + Add node**, paste the three values, click **Save & connect**. Done — switch to the new node in the sidebar and create servers on the VPS as if they were local.

### 3 · Optional: bring your own Let's Encrypt cert

If your VPS has a real domain, run `certbot` first and pass the PEM paths to the installer:

```bash
sudo certbot certonly --standalone -d node.example.com
sudo localforge-agent install \
  --cert-pem /etc/letsencrypt/live/node.example.com/fullchain.pem \
  --key-pem  /etc/letsencrypt/live/node.example.com/privkey.pem \
  --bind 0.0.0.0
```

The pairing summary then skips the fingerprint line — the desktop trusts the CA-signed cert via the system WebPKI roots.

---

## 🏗️ Architecture

```
                ┌─────────────────────────────┐
                │ LocalForge Desktop (Tauri)  │
                │ React + Rust + Zustand      │
                └──────┬──────────────────────┘
                       │ NodeBackend trait
       ┌───────────────┼──────────────────┐
       │                                  │
       ▼ "local"                          ▼ "remote"
  bollard ──→ /var/run/docker.sock   HTTPS+WSS (cert-pinned)
                                          │
                              ┌───────────▼───────────┐
                              │  localforge-agent     │
                              │  axum + rustls        │
                              │  bearer auth          │
                              └──────┬────────────────┘
                                     │
                                     ▼
                              bollard ──→ docker.sock
                                          (on the VPS)
```

5-crate Cargo workspace:

| Crate | Role |
|-------|------|
| `localforge-core` | Shared types + `NodeBackend` trait + game catalogue |
| `localforge-backend-local` | bollard adapter (used by desktop **and** agent) |
| `localforge-backend-remote` | HTTPS+WSS client implementing `NodeBackend` |
| `localforge-agent` | axum daemon — exposes `NodeBackend` over `/v1/*` |
| `localforge` (desktop) | Tauri shell around the trait |

Everything routes through one `NodeBackend` impl — local and remote behave identically end-to-end.

---

## 🧰 Tech stack

- **Frontend**: React 19 + TypeScript 6 + Tailwind 4 (CSS-first config) + Vite 8 (Rolldown) + Zustand
- **Desktop**: Tauri 2
- **Rust**: tokio + bollard + axum + rustls + sysinfo
- **Auth**: bearer token over HTTPS; cert pinning via SHA-256 fingerprint or WebPKI roots
- **Streaming**: tokio-tungstenite (logs + install events), reqwest body streams (file transfers)

---

## 🛠️ Build from source

Prerequisites:
- Node.js 20+
- Rust 1.95+ stable
- On Linux: `libwebkit2gtk-4.1-dev libayatana-appindicator3-dev librsvg2-dev libssl-dev libgtk-3-dev patchelf`
- On Windows: Visual Studio 2022 Build Tools (C++ workload) + WebView2 Runtime
- On macOS: Xcode Command Line Tools

```bash
git clone https://github.com/fabri2000779/localforge
cd localforge
npm install
npm run tauri:dev        # development with HMR
npm run tauri:build      # production build (installer in src-tauri/target/release/bundle)
```

To build only the agent for a single target:
```bash
cd src-tauri
cargo build --release -p localforge-agent --target x86_64-unknown-linux-gnu
```

---

## 📁 Project structure

```
localforge/
├── src/                      # React frontend
│   ├── components/           # UI components (NodeSelector, FileManager, AddNodeWizard…)
│   ├── pages/                # Routes (Home, Servers, Nodes, Games, Settings…)
│   ├── stores/               # Zustand stores (nodesStore, serverStore, dockerStore…)
│   └── types/                # Shared TypeScript types
├── src-tauri/                # Rust workspace
│   ├── core/                 # localforge-core
│   ├── backend-local/        # localforge-backend-local (bollard impl)
│   ├── backend-remote/       # localforge-backend-remote (HTTPS+WSS client)
│   ├── agent/                # localforge-agent (axum daemon)
│   └── src/                  # localforge desktop crate
├── scripts/
│   ├── install-agent.sh      # Linux VPS installer
│   └── install-agent.ps1     # Windows VPS installer
└── .github/workflows/
    └── release.yml           # Multi-platform CI: builds, signs, notarizes, attaches all assets
```

Data lives at:
- `~/LocalForge/` on desktop
- `/var/lib/localforge/` on Linux agents
- `C:\ProgramData\LocalForge\` on Windows agents

---

## 🙏 Credits

- Built on [Tauri](https://tauri.app/), [axum](https://github.com/tokio-rs/axum), [bollard](https://github.com/fussybeaver/bollard), and [rustls](https://github.com/rustls/rustls).

## 📄 License

[MIT](LICENSE)
