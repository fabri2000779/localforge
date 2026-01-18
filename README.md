# Serverwave Anywhere 🎮

**Run Serverwave game servers anywhere, locally.**

Serverwave Anywhere lets you run the same high-quality game server images used by Serverwave cloud, right on your own PC. No terminal commands, no Docker knowledge required.

## Features

- **One-Click Setup** - Select a game, click "Create Server", done
- **Docker-Powered** - Same images as Serverwave cloud
- **Persistent Storage** - Your worlds and configs stay on your PC
- **Built-in Console** - View logs and send commands from the app
- **Multi-Game Support** - Minecraft, Valheim, Terraria, Hytale, and more
- **Fully Offline** - No internet required after initial setup

## Supported Games

| Game | Status | Notes |
|------|--------|-------|
| Minecraft Java | ✅ Ready | Paper, Vanilla, Forge, Fabric |
| Minecraft Bedrock | ✅ Ready | Official Bedrock server |
| Hytale | ✅ Ready | Serverwave optimized image |
| Valheim | ✅ Ready | |
| Terraria | ✅ Ready | |
| Factorio | ✅ Ready | |
| 7 Days to Die | ✅ Ready | |

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
# Install dependencies
npm install

# Run in development mode
npm run tauri dev

# Build for production
npm run tauri build
```

## Architecture

```
┌─────────────────────────────────────────────────────────┐
│                Serverwave Anywhere UI                   │
│                  (React + TypeScript)                   │
├─────────────────────────────────────────────────────────┤
│                    Tauri Bridge                         │
│              (IPC Commands & Events)                    │
├─────────────────────────────────────────────────────────┤
│                   Rust Backend                          │
│  ┌─────────────┐ ┌─────────────┐ ┌─────────────────┐   │
│  │   Docker    │ │   Server    │ │   Config        │   │
│  │   Manager   │ │   Process   │ │   Manager       │   │
│  └─────────────┘ └─────────────┘ └─────────────────┘   │
├─────────────────────────────────────────────────────────┤
│                    Docker Engine                        │
│  ┌─────────────┐ ┌─────────────┐ ┌─────────────────┐   │
│  │  Minecraft  │ │  Valheim    │ │   Hytale        │   │
│  │  Container  │ │  Container  │ │   Container     │   │
│  └─────────────┘ └─────────────┘ └─────────────────┘   │
├─────────────────────────────────────────────────────────┤
│                 User's File System                      │
│        ~/ServerWaveAnywhere/servers/{game}/{id}/        │
└─────────────────────────────────────────────────────────┘
```

## Project Structure

```
serverwave-anywhere/
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

## License

MIT
