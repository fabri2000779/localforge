/** Dev-only Tauri runtime mock so the UI renders in a plain browser; installed only when `import.meta.env.DEV`. */

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

    case 'cloud_templates_list':
      return { templates: [], nextBefore: null, nextBeforeId: null };
    case 'cloud_node_list':
    case 'cloud_list_machines':
    case 'query_crash_events':
      return [];
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

    // Window plugin invocations return null so the TitleBar buttons don't crash.
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

  // The event API calls this from every unlisten(); a no-op keeps the dev console quiet.
  (window as unknown as { __TAURI_EVENT_PLUGIN_INTERNALS__?: unknown }).__TAURI_EVENT_PLUGIN_INTERNALS__ = {
    unregisterListener: () => {},
  };
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
      void callback;
      return Math.floor(Math.random() * 1e9);
    },
    convertFileSrc: (path: string) => path,
  };

  console.info('[dev-shim] Tauri runtime mocked — no real backend calls');
}

// import.meta.env.DEV is a build-time literal, so the shim is eliminated from production bundles.
if (import.meta.env.DEV) {
  installShim();
}
