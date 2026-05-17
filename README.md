# LocalForge 🎮

**Run game servers locally with a single click.**

LocalForge spins up Minecraft, Valheim, Rust and other game servers on your own PC using the same battle-tested container images as Pelican Panel. No terminal commands, no Docker knowledge required.

## Features

- **One-Click Setup** - Select a game, click "Create Server", done
- **Docker-Powered** - Built on `ghcr.io/parkervcp/yolks` (Pelican Panel's images)
- **Persistent Storage** - Worlds and configs stay on your PC
- **Built-in Console** - View logs and send commands from the app
- **Multi-Game Support** - Minecraft, Valheim, Terraria, Hytale, Rust, Palworld and more
- **Fully Offline** - No internet required after initial setup

## Supported Games

| Game | Status | Notes |
|------|--------|-------|
| Minecraft Java | ✅ Ready | Paper, Vanilla, Forge, Fabric |
| Minecraft Bedrock | ✅ Ready | Official Bedrock server |
| Hytale | ✅ Ready | |
| Valheim | ✅ Ready | |
| Terraria | ✅ Ready | |
| Factorio | ✅ Ready | |
| 7 Days to Die | ✅ Ready | |
| Rust | ✅ Ready | |
| Palworld | ✅ Ready | |
| Sons of the Forest | ✅ Ready | Wine-based |

## Tech Stack

- **Frontend**: React + TypeScript + Tailwind CSS
- **Backend**: Rust (Tauri)
- **Containerization**: Docker
- **State Management**: Zustand

## Prerequisites

- [Docker Desktop](https://www.docker.com/products/docker-desktop/) installed and running
- 4GB+ RAM recommended
- Ports available (varies by game)

## Development

```bash
npm install
npm run tauri:dev      # development mode
npm run tauri:build    # production build
```

## Architecture

```
┌─────────────────────────────────────────────────────────┐
│                      LocalForge UI                       │
│                  (React + TypeScript)                    │
├─────────────────────────────────────────────────────────┤
│                     Tauri Bridge                         │
│               (IPC Commands & Events)                    │
├─────────────────────────────────────────────────────────┤
│                     Rust Backend                         │
│  ┌─────────────┐ ┌─────────────┐ ┌─────────────────┐    │
│  │   Docker    │ │   Server    │ │   Config        │    │
│  │   Manager   │ │   Process   │ │   Manager       │    │
│  └─────────────┘ └─────────────┘ └─────────────────┘    │
├─────────────────────────────────────────────────────────┤
│                    Docker Engine                         │
│  ┌─────────────┐ ┌─────────────┐ ┌─────────────────┐    │
│  │  Minecraft  │ │  Valheim    │ │   Rust          │    │
│  │  Container  │ │  Container  │ │   Container     │    │
│  └─────────────┘ └─────────────┘ └─────────────────┘    │
├─────────────────────────────────────────────────────────┤
│                  User's File System                      │
│          ~/LocalForge/servers/{game}/{id}/               │
└─────────────────────────────────────────────────────────┘
```

## Project Structure

```
localforge/
├── src/                    # React frontend
│   ├── components/         # UI components
│   ├── pages/              # Page components
│   ├── stores/             # Zustand stores
│   └── types/              # TypeScript types
├── src-tauri/              # Rust backend
│   ├── src/
│   │   ├── commands/       # Tauri commands
│   │   ├── docker/         # Docker management
│   │   ├── games/          # Game definitions
│   │   └── main.rs         # Entry point
│   └── Cargo.toml
└── package.json
```

## Credits

Container images by [Parker Vincent](https://github.com/parkervcp) and the [Pelican Panel](https://pelican.dev/) community.

## License

MIT
