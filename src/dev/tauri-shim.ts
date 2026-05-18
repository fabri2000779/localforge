/**
 * Dev-only mock for the Tauri runtime. When you run `vite` outside the
 * Tauri shell (e.g. for visual debugging in a regular browser), the
 * `@tauri-apps/api/core` `invoke` helper crashes because
 * `window.__TAURI_INTERNALS__` is undefined. The components that call
 * `getCurrentWindow().minimize()` etc. also throw, which propagates
 * up and renders the whole app as a blank black screen.
 *
 * Install lightweight stubs on `window.__TAURI_INTERNALS__` so the
 * frontend can render with realistic mock data. NOT imported in
 * production builds — only in `import.meta.env.DEV` mode from
 * main.tsx.
 */

type AnyArgs = Record<string, unknown>;

interface InvokeOptions {
  headers?: Record<string, string>;
}

const sampleGames = [
  {
    game_type: 'minecraft',
    name: 'Minecraft',
    description: 'Vanilla Minecraft Java edition server.',
    icon: '🧱',
    logo_url: null,
    min_ram_mb: 1024,
    recommended_ram_mb: 4096,
    image: 'ghcr.io/parkervcp/yolks:java_21',
    is_custom: false,
    variables: [],
    ports: [{ name: 'game', container_port: 25565, protocol: 'tcp' }],
    config_files: [],
  },
  {
    game_type: 'palworld',
    name: 'Palworld',
    description: 'Open-world survival game with creature collection.',
    icon: '🐾',
    logo_url: null,
    min_ram_mb: 8192,
    recommended_ram_mb: 16384,
    image: 'parkervcp/games:palworld',
    is_custom: false,
    variables: [],
    ports: [{ name: 'game', container_port: 8211, protocol: 'udp' }],
    config_files: [],
  },
  {
    game_type: 'valheim',
    name: 'Valheim',
    description: 'Viking-themed survival sandbox.',
    icon: '⚔️',
    logo_url: null,
    min_ram_mb: 2048,
    recommended_ram_mb: 4096,
    image: 'parkervcp/games:valheim',
    is_custom: false,
    variables: [],
    ports: [{ name: 'game', container_port: 2456, protocol: 'udp' }],
    config_files: [],
  },
  {
    game_type: 'rust',
    name: 'Rust',
    description: 'Multiplayer survival on a procedural island.',
    icon: '🔥',
    logo_url: null,
    min_ram_mb: 4096,
    recommended_ram_mb: 8192,
    image: 'parkervcp/games:rust',
    is_custom: false,
    variables: [],
    ports: [{ name: 'game', container_port: 28015, protocol: 'udp' }],
    config_files: [],
  },
];

function handleCommand(cmd: string, _args: AnyArgs): unknown {
  switch (cmd) {
    case 'check_docker_status':
      return { available: true, running: true, error: null };

    case 'get_docker_info':
      return {
        version: '29.4.3',
        api_version: '1.54',
        os: 'Docker Desktop (mocked)',
        arch: 'x86_64',
        containers_running: 0,
        containers_total: 0,
        images: 0,
      };

    case 'list_nodes':
      return [
        {
          id: 'local',
          label: 'This machine',
          kind: { kind: 'local' },
        },
      ];

    case 'cluster_summary':
      return {
        total_nodes: 1,
        online_nodes: 1,
        containers_running: 0,
        containers_total: 0,
        images: 0,
      };

    case 'get_node_stats':
      return null;

    case 'list_servers':
      return [];

    case 'get_server':
      return null;

    case 'list_available_games':
      return sampleGames;

    case 'get_games_config_path':
      return '/mock/config/path';

    case 'check_needs_install':
      return false;

    case 'get_server_stats':
    case 'get_server_disk_usage':
      return null;

    case 'agent_install_command':
      return {
        linux:
          "curl -sSL https://github.com/fabri2000779/localforge/releases/download/latest/install-agent.sh | sudo bash",
        windows:
          "iex \"& { $(irm https://github.com/fabri2000779/localforge/releases/download/latest/install-agent.ps1) }\"",
      };

    // Window plugin invocations (minimize, maximize, etc.) all return
    // null successfully so the TitleBar buttons don't crash.
    default:
      if (cmd.startsWith('plugin:')) return null;
      console.debug('[dev-shim] unhandled invoke', cmd);
      return null;
  }
}

function installShim() {
  if (typeof window === 'undefined') return;
  // Skip when running inside the real Tauri shell.
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  if ((window as any).__TAURI_INTERNALS__) return;

  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  (window as any).__TAURI_INTERNALS__ = {
    metadata: {
      currentWindow: { label: 'main' },
      currentWebview: { label: 'main', windowLabel: 'main' },
    },
    invoke: (cmd: string, args: AnyArgs = {}, _opts?: InvokeOptions) => {
      try {
        return Promise.resolve(handleCommand(cmd, args));
      } catch (e) {
        return Promise.reject(e);
      }
    },
    transformCallback: (callback?: (response: unknown) => void) => {
      // Return a numeric id; Tauri uses these to map responses back to
      // callbacks. We never call the callback so just give a stable id.
      void callback;
      return Math.floor(Math.random() * 1e9);
    },
    convertFileSrc: (path: string) => path,
  };

  console.info('[dev-shim] Tauri runtime mocked — no real backend calls');
}

// Vite replaces `import.meta.env.DEV` with the literal `true` / `false`
// at build time, so the entire shim call is dead-code-eliminated from
// production bundles. In dev (npm run dev) the shim runs and provides
// stub Tauri responses for visual debugging.
if (import.meta.env.DEV) {
  installShim();
}
