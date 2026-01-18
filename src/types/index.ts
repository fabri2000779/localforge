// TypeScript types for Serverwave Anywhere

export type GameType = string;

export type ServerStatus =
  | 'stopped'
  | 'starting'
  | 'installing'
  | 'running'
  | 'stopping'
  | 'error';

export interface Server {
  id: string;
  name: string;
  game_type: GameType;
  status: ServerStatus;
  container_id: string | null;
  port: number;
  memory_mb: number;
  data_path: string;
  created_at: string;
  config: Record<string, string>;
  installed: boolean;
  install_container_id?: string;
}

export type PortProtocol = 'tcp' | 'udp' | 'both';

export interface PortConfig {
  container_port: number;
  protocol: PortProtocol;
  description?: string;
  env_var?: string; // Environment variable that maps to this port
}

export type SystemMapping = 'none' | 'ram' | 'port';
export type FieldType = 'text' | 'number' | 'password' | 'select';

export interface SelectOption {
  value: string;
  label: string;
}

export interface Variable {
  env: string;
  name: string;
  description: string;
  default: string;
  system_mapping?: SystemMapping;
  user_editable: boolean;
  options?: SelectOption[];
  field_type: FieldType;
}

export type ConfigFileFormat = 'json' | 'yaml' | 'properties';

export interface ConfigFile {
  path: string;
  format: ConfigFileFormat;
  variables: Record<string, string>;
}

export interface GameConfig {
  game_type: GameType;
  name: string;
  description: string;
  docker_image: string;
  startup: string;
  stop_command: string;
  variables: Variable[];
  ports: PortConfig[];
  volume_path: string;
  min_ram_mb: number;
  recommended_ram_mb: number;
  icon: string;
  logo_url?: string;
  install_script?: string;
  install_image?: string;
  config_files: ConfigFile[];
  is_custom: boolean;
  console: boolean;
}

export interface DockerStatus {
  available: boolean;
  running: boolean;
  error: string | null;
}

export interface DockerInfo {
  version: string;
  api_version: string;
  os: string;
  arch: string;
  containers_running: number;
  containers_total: number;
  images: number;
}

export interface ServerResponse {
  success: boolean;
  server: Server | null;
  error: string | null;
}

export interface LogsResponse {
  logs: string[];
  error: string | null;
}

export interface CreateServerRequest {
  name: string;
  game_type: GameType;
  port?: number;
  config?: Record<string, string>;
  memory_mb?: number;
}

export const DEFAULT_GAME_CONFIG: GameConfig = {
  game_type: '',
  name: '',
  description: '',
  docker_image: '',
  startup: '',
  stop_command: '',
  variables: [],
  ports: [{ container_port: 25565, protocol: 'tcp' }],
  volume_path: '/data',
  min_ram_mb: 512,
  recommended_ram_mb: 2048,
  icon: '🎮',
  config_files: [],
  is_custom: true,
  console: true,
};
