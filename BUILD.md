# Building Serverwave Anywhere

## Development

```bash
npm install
npm run tauri:dev
```

## Production Build

### Windows (creates .msi + .exe installers)

```bash
npm run tauri:build
```

Output: `src-tauri/target/release/bundle/nsis/Serverwave Anywhere_0.1.0_x64-setup.exe`

### macOS (creates .dmg + .app)

```bash
npm run tauri:build
```

Output: `src-tauri/target/release/bundle/dmg/`

### Linux (creates .deb, .AppImage, .rpm)

```bash
npm run tauri:build
```

Output: `src-tauri/target/release/bundle/`

## Build for Specific Target

```bash
# Windows only
npm run tauri:build -- --target x86_64-pc-windows-msvc

# macOS Intel
npm run tauri:build -- --target x86_64-apple-darwin

# macOS Apple Silicon
npm run tauri:build -- --target aarch64-apple-darwin

# Linux
npm run tauri:build -- --target x86_64-unknown-linux-gnu
```

## Cross-Platform Builds

Build on the target platform:
- Windows builds → Build on Windows
- macOS builds → Build on macOS
- Linux builds → Build on Linux

For CI/CD, use GitHub Actions (see `.github/workflows/release.yml`).

## Prerequisites

### Windows
- Visual Studio Build Tools 2019+ with C++ workload
- WebView2 (pre-installed on Windows 10/11)

### macOS
- Xcode Command Line Tools: `xcode-select --install`

### Linux
- Build essentials: `sudo apt install build-essential`
- WebKit: `sudo apt install libwebkit2gtk-4.1-dev`
- Other deps: `sudo apt install libssl-dev libgtk-3-dev libayatana-appindicator3-dev librsvg2-dev`

## Troubleshooting

### "WebView2 not found" on Windows
Download from: https://developer.microsoft.com/en-us/microsoft-edge/webview2/

### Build fails with "link.exe not found"
Install Visual Studio Build Tools with C++ workload.

### macOS "app is damaged" error
Allow in System Preferences > Security.
