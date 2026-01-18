// Server store using Zustand

import { create } from 'zustand';
import { invoke } from '@tauri-apps/api/core';
import { listen, UnlistenFn } from '@tauri-apps/api/event';
import type {
  Server,
  CreateServerRequest,
  ServerResponse,
  LogsResponse,
} from '../types';

interface LogEvent {
  server_id: string;
  line: string;
}

interface ContainerStats {
  cpu_percent: number;
  memory_usage_mb: number;
  memory_limit_mb: number;
  memory_percent: number;
}

interface ServerState {
  servers: Server[];
  selectedServer: Server | null;
  isLoading: boolean;
  error: string | null;
  logs: string[];
  stats: ContainerStats | null;
  logUnlisten: UnlistenFn | null;
  statsInterval: number | null;
  isStreaming: boolean;

  fetchServers: () => Promise<void>;
  createServer: (request: CreateServerRequest) => Promise<Server | null>;
  startServer: (serverId: string) => Promise<void>;
  stopServer: (serverId: string) => Promise<void>;
  deleteServer: (serverId: string, deleteData?: boolean) => Promise<void>;
  updateServerConfig: (serverId: string, config: Record<string, string>) => Promise<boolean>;
  reinstallServer: (serverId: string) => Promise<void>;
  updateServerGame: (serverId: string) => Promise<void>;
  checkNeedsInstall: (serverId: string) => Promise<boolean>;
  selectServer: (server: Server | null) => void;
  sendCommand: (serverId: string, command: string) => Promise<string | null>;
  fetchLogs: (serverId: string) => Promise<void>;
  fetchStats: (serverId: string) => Promise<void>;
  attachToServer: (serverId: string) => Promise<void>;
  detachFromServer: (serverId: string) => Promise<void>;
  startStatsPolling: (serverId: string) => void;
  stopStatsPolling: () => void;
  clearError: () => void;
  clearLogs: () => void;
}

export const useServerStore = create<ServerState>((set, get) => ({
  servers: [],
  selectedServer: null,
  isLoading: false,
  error: null,
  logs: [],
  stats: null,
  logUnlisten: null,
  statsInterval: null,
  isStreaming: false,

  fetchServers: async () => {
    set({ isLoading: true, error: null });
    try {
      const servers = await invoke<Server[]>('list_servers');
      set({ servers, isLoading: false });
      const selected = get().selectedServer;
      if (selected) {
        const updated = servers.find((s) => s.id === selected.id);
        if (updated) set({ selectedServer: updated });
      }
    } catch (error) {
      console.error('[Store] fetchServers error:', error);
      set({ error: String(error), isLoading: false });
    }
  },

  createServer: async (request) => {
    set({ isLoading: true, error: null });
    try {
      const response = await invoke<ServerResponse>('create_server', { request });
      if (response.success && response.server) {
        await get().fetchServers();
        set({ isLoading: false });
        return response.server;
      } else {
        set({ error: response.error || 'Failed to create server', isLoading: false });
        return null;
      }
    } catch (error) {
      set({ error: String(error), isLoading: false });
      return null;
    }
  },

  startServer: async (serverId) => {
    set({ isLoading: true, error: null, logs: [] });
    try {
      await get().attachToServer(serverId);
      await invoke<ServerResponse>('start_server', { serverId });
      await get().fetchServers();
      get().startStatsPolling(serverId);
      set({ isLoading: false });
    } catch (error) {
      console.error('[Store] startServer error:', error);
      set({ error: String(error), isLoading: false });
    }
  },

  stopServer: async (serverId) => {
    set({ isLoading: true, error: null });
    try {
      get().stopStatsPolling();
      await invoke<ServerResponse>('stop_server', { serverId });
      await get().detachFromServer(serverId);
      await get().fetchServers();
      set({ isLoading: false, stats: null, isStreaming: false });
    } catch (error) {
      set({ error: String(error), isLoading: false });
    }
  },

  deleteServer: async (serverId, deleteData = true) => {
    set({ isLoading: true, error: null });
    try {
      get().stopStatsPolling();
      await get().detachFromServer(serverId);
      await invoke<ServerResponse>('delete_server', { serverId, deleteData });
      const selected = get().selectedServer;
      if (selected?.id === serverId) set({ selectedServer: null });
      await get().fetchServers();
      set({ isLoading: false });
    } catch (error) {
      set({ error: String(error), isLoading: false });
    }
  },

  updateServerConfig: async (serverId, config) => {
    try {
      const response = await invoke<ServerResponse>('update_server_config', { serverId, config });
      if (response.success) {
        await get().fetchServers();
        return true;
      }
      return false;
    } catch (error) {
      console.error('[Store] updateServerConfig error:', error);
      set({ error: String(error) });
      return false;
    }
  },

  reinstallServer: async (serverId) => {
    set({ isLoading: true, error: null, logs: [] });
    try {
      await get().attachToServer(serverId);
      await invoke<ServerResponse>('reinstall_server', { serverId });
      await get().fetchServers();
      set({ isLoading: false });
    } catch (error) {
      console.error('[Store] reinstallServer error:', error);
      set({ error: String(error), isLoading: false });
    }
  },

  updateServerGame: async (serverId) => {
    set({ isLoading: true, error: null, logs: [] });
    try {
      await get().attachToServer(serverId);
      await invoke<ServerResponse>('update_server_game', { serverId });
      await get().fetchServers();
      set({ isLoading: false });
    } catch (error) {
      console.error('[Store] updateServerGame error:', error);
      set({ error: String(error), isLoading: false });
    }
  },

  checkNeedsInstall: async (serverId) => {
    try {
      return await invoke<boolean>('check_needs_install', { serverId });
    } catch (error) {
      console.error('[Store] checkNeedsInstall error:', error);
      return false;
    }
  },

  selectServer: (server) => set({ selectedServer: server }),

  sendCommand: async (serverId, command) => {
    try {
      const result = await invoke<string>('send_command', { serverId, command });
      return result;
    } catch (error) {
      console.error('[Store] sendCommand error:', error);
      set({ error: String(error) });
      return null;
    }
  },

  fetchLogs: async (serverId) => {
    try {
      const response = await invoke<LogsResponse>('get_server_logs', { serverId, lines: 500 });
      set({ logs: response.logs });
    } catch (error) {
      console.error('[Store] fetchLogs error:', error);
    }
  },

  fetchStats: async (serverId) => {
    try {
      const stats = await invoke<ContainerStats>('get_server_stats', { serverId });
      set({ stats });
    } catch (error) {
      // Silently ignore stats errors
    }
  },

  attachToServer: async (serverId) => {
    console.log('[Store] attachToServer called:', serverId);
    
    const { logUnlisten } = get();
    if (logUnlisten) {
      logUnlisten();
      set({ logUnlisten: null });
    }

    try {
      const unlisten = await listen<LogEvent>('server-log', (event) => {
        if (event.payload.server_id === serverId) {
          set((state) => ({ logs: [...state.logs, event.payload.line] }));
        }
      });

      set({ logUnlisten: unlisten, isStreaming: true });

      console.log('[Store] Invoking attach_server...');
      await invoke('attach_server', { serverId });
      console.log('[Store] attach_server complete');
    } catch (error) {
      console.error('[Store] attachToServer error:', error);
      set({ isStreaming: false });
    }
  },

  detachFromServer: async (serverId) => {
    const { logUnlisten } = get();
    if (logUnlisten) {
      logUnlisten();
      set({ logUnlisten: null, isStreaming: false });
    }
    try {
      await invoke('detach_server', { serverId });
    } catch (error) {
      // Ignore
    }
  },

  startStatsPolling: (serverId) => {
    get().stopStatsPolling();
    get().fetchStats(serverId);
    const interval = window.setInterval(() => get().fetchStats(serverId), 2000);
    set({ statsInterval: interval });
  },

  stopStatsPolling: () => {
    const { statsInterval } = get();
    if (statsInterval) {
      clearInterval(statsInterval);
      set({ statsInterval: null });
    }
  },

  clearError: () => set({ error: null }),
  clearLogs: () => set({ logs: [] }),
}));
