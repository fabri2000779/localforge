// Server store using Zustand.
//
// Every Tauri invoke that touches a Docker daemon takes a `nodeId`
// argument (defaulting to "local" on the Rust side). We always read the
// active node id from nodesStore so switching nodes in the sidebar
// automatically scopes every subsequent action to the new node.

import { create } from 'zustand';
import { invoke } from '@tauri-apps/api/core';
import { listen, UnlistenFn } from '@tauri-apps/api/event';
import type {
  Server,
  CreateServerRequest,
  ServerResponse,
  LogsResponse,
} from '../types';
import { useNodesStore } from './nodesStore';
import { emitAudit } from '../utils/audit';
import { routeServerAction } from '../utils/subUser';

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
  updateServerConfig: (
    serverId: string,
    config: Record<string, string>,
  ) => Promise<boolean>;
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

/// Read the currently-active node id from nodesStore at the moment of
/// the invoke. We don't subscribe — each call gets a fresh value so
/// switching nodes mid-flight doesn't strand stale calls.
const currentNodeId = () => useNodesStore.getState().activeNodeId;

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
      const servers = await invoke<Server[]>('list_servers', {
        nodeId: currentNodeId(),
      });
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
      const response = await invoke<ServerResponse>('create_server', {
        request,
        nodeId: currentNodeId(),
      });
      if (response.success && response.server) {
        await get().fetchServers();
        set({ isLoading: false });
        return response.server;
      } else {
        set({
          error: response.error || 'Failed to create server',
          isLoading: false,
        });
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
      const route = await routeServerAction('server.start', { serverId });
      if (route.via === 'local') {
        await get().attachToServer(serverId);
        await invoke<ServerResponse>('start_server', {
          serverId,
          nodeId: currentNodeId(),
        });
        get().startStatsPolling(serverId);
        await get().fetchServers();
      }
      // Sub-user mode: cmd is fire-and-forget through the relay. The
      // owner's RelayCommandExecutor runs `start_server` locally and
      // broadcasts an `event` we'll surface as a toast in a future tier.
      emitAudit('server.start', serverId);
      set({ isLoading: false });
    } catch (error) {
      console.error('[Store] startServer error:', error);
      set({ error: String(error), isLoading: false });
    }
  },

  stopServer: async (serverId) => {
    set({ isLoading: true, error: null });
    try {
      const route = await routeServerAction('server.stop', { serverId });
      if (route.via === 'local') {
        get().stopStatsPolling();
        await invoke<ServerResponse>('stop_server', {
          serverId,
          nodeId: currentNodeId(),
        });
        await get().detachFromServer(serverId);
        await get().fetchServers();
        set({ stats: null, isStreaming: false });
      }
      emitAudit('server.stop', serverId);
      set({ isLoading: false });
    } catch (error) {
      set({ error: String(error), isLoading: false });
    }
  },

  deleteServer: async (serverId, deleteData = true) => {
    set({ isLoading: true, error: null });
    try {
      get().stopStatsPolling();
      await get().detachFromServer(serverId);
      await invoke<ServerResponse>('delete_server', {
        serverId,
        deleteData,
        nodeId: currentNodeId(),
      });
      emitAudit('server.delete', serverId, { deleteData });
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
      const response = await invoke<ServerResponse>('update_server_config', {
        serverId,
        config,
        nodeId: currentNodeId(),
      });
      if (response.success) {
        emitAudit('server.update_config', serverId);
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
      await invoke<ServerResponse>('reinstall_server', {
        serverId,
        nodeId: currentNodeId(),
      });
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
      await invoke<ServerResponse>('update_server_game', {
        serverId,
        nodeId: currentNodeId(),
      });
      await get().fetchServers();
      set({ isLoading: false });
    } catch (error) {
      console.error('[Store] updateServerGame error:', error);
      set({ error: String(error), isLoading: false });
    }
  },

  checkNeedsInstall: async (serverId) => {
    try {
      return await invoke<boolean>('check_needs_install', {
        serverId,
        nodeId: currentNodeId(),
      });
    } catch (error) {
      console.error('[Store] checkNeedsInstall error:', error);
      return false;
    }
  },

  selectServer: (server) => set({ selectedServer: server }),

  sendCommand: async (serverId, command) => {
    try {
      const route = await routeServerAction('server.send_command', { serverId, command });
      let result = '';
      if (route.via === 'local') {
        result = await invoke<string>('send_command', {
          serverId,
          command,
          nodeId: currentNodeId(),
        });
      }
      // Truncate the command in metadata — full text could contain
      // sensitive paths / args. The audit's purpose is "Bob ran a
      // console cmd at 12:34", not auditable transcript.
      emitAudit('server.send_command', serverId, {
        command_preview: command.slice(0, 60),
      });
      return result;
    } catch (error) {
      console.error('[Store] sendCommand error:', error);
      set({ error: String(error) });
      return null;
    }
  },

  fetchLogs: async (serverId) => {
    try {
      const response = await invoke<LogsResponse>('get_server_logs', {
        serverId,
        lines: 500,
        nodeId: currentNodeId(),
      });
      set({ logs: response.logs });
    } catch (error) {
      console.error('[Store] fetchLogs error:', error);
    }
  },

  fetchStats: async (serverId) => {
    try {
      const stats = await invoke<ContainerStats>('get_server_stats', {
        serverId,
        nodeId: currentNodeId(),
      });
      set({ stats });
    } catch {
      // Silently ignore stats errors
    }
  },

  attachToServer: async (serverId) => {
    const { logUnlisten } = get();
    if (logUnlisten) {
      logUnlisten();
      set({ logUnlisten: null });
    }

    try {
      // The xterm listener subscribes to local `server-log` Tauri events
      // regardless of who emitted them. In sub-user mode, the
      // RelayLogBridge re-emits relay `console_line` events as local
      // `server-log` events, so this single listener feeds both modes.
      const unlisten = await listen<LogEvent>('server-log', (event) => {
        if (event.payload.server_id === serverId) {
          set((state) => ({ logs: [...state.logs, event.payload.line] }));
        }
      });

      set({ logUnlisten: unlisten, isStreaming: true });

      // Route the actual "start the stream" call. Owner: local Tauri
      // command kicks Docker's log reader. Sub-user: relay cmd asks
      // the owner's RelayCommandExecutor to do the same on their
      // hardware, and the owner's RelayLogBridge forwards each line.
      const route = await routeServerAction('server.start' /* unused */, { serverId });
      // The route helper signature requires an ActionKind we have in
      // the map; reuse the more-specific attach for clarity.
      if (route.via === 'local') {
        await invoke('attach_server', {
          serverId,
          nodeId: currentNodeId(),
        });
      } else {
        // Send the attach cmd explicitly — we re-used routeServerAction
        // above just to know which path we're on.
        await invoke('cloud_relay_send_cmd', {
          payload: {
            type: 'cmd',
            cmd: 'server.attach',
            target: serverId,
            request_id: `attach.${serverId}.${Date.now()}`,
          },
        });
      }
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
      const route = await routeServerAction('server.stop' /* unused */, { serverId });
      if (route.via === 'local') {
        await invoke('detach_server', { serverId });
      } else {
        await invoke('cloud_relay_send_cmd', {
          payload: {
            type: 'cmd',
            cmd: 'server.detach',
            target: serverId,
            request_id: `detach.${serverId}.${Date.now()}`,
          },
        });
      }
    } catch {
      // Ignore — best-effort.
    }
  },

  startStatsPolling: (serverId) => {
    get().stopStatsPolling();
    get().fetchStats(serverId);
    const interval = window.setInterval(
      () => get().fetchStats(serverId),
      2000,
    );
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

// Re-fetch the server list whenever the active node changes so the UI
// always reflects the servers on the node the user is currently looking
// at. Detach any active log stream first — it belongs to the old node.
let lastActive = useNodesStore.getState().activeNodeId;
useNodesStore.subscribe((state) => {
  if (state.activeNodeId !== lastActive) {
    lastActive = state.activeNodeId;
    const store = useServerStore.getState();
    if (store.logUnlisten) {
      store.logUnlisten();
    }
    store.stopStatsPolling();
    useServerStore.setState({
      servers: [],
      selectedServer: null,
      logs: [],
      stats: null,
      logUnlisten: null,
      isStreaming: false,
    });
    store.fetchServers();
  }
});
