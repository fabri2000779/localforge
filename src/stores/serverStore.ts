// Server store. Every Docker-touching invoke takes the active node id from nodesStore at call time.

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
import { isRelayRouted, routeServerAction } from '../utils/subUser';

interface LogEvent {
  server_id: string;
  line: string;
}

// Cap for in-memory console buffers (oldest lines dropped).
const MAX_CONSOLE_LINES = 2000;

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
  stopServer: (serverId: string) => Promise<boolean>;
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

// Read at invoke time (not subscribed) so a node switch mid-flight can't strand stale calls.
const currentNodeId = () => useNodesStore.getState().activeNodeId;

// Monotonic token for attach/detach ordering (see attachToServer).
let attachGeneration = 0;

// Monotonic token: a slow list from a previous node must not overwrite the current node's list.
let fetchGeneration = 0;

// Best-effort push of the LOCAL node's servers after a config-changing mutation; rejects when signed out.
function autoSyncToCloud() {
  void invoke('cloud_sync_now').catch(() => {
    /* not signed in / sync not set up — nothing to push */
  });
}

// Tombstone a deleted server in the cloud (the push only upserts what still exists). Best-effort.
export function tombstoneInCloud(serverId: string) {
  void invoke('cloud_sync_delete_server', { serverId }).catch(() => {
    /* not signed in / never synced — nothing to tombstone */
  });
}

// Newest crash-journal ts already pushed; seeded on first read so pre-existing crashes never re-alert.
let lastCrashEventTs: number | null = null;

// Wire shape of core::CrashEvent (camelCase).
interface CrashJournalEvent {
  ts: number;
  serverId: string;
  /** 'crashed' | 'restarted' | 'backoff' */
  kind: string;
}

// Push a cloud crash alert for each NEW 'crashed'/'backoff' journal event ('restarted' needs none).
async function checkCrashJournal() {
  let events: CrashJournalEvent[];
  try {
    events = await invoke<CrashJournalEvent[]>('query_crash_events', {
      serverId: null,
      limit: 50,
    });
  } catch {
    return;
  }
  if (events.length === 0) return;
  const newest = events.reduce((m, e) => Math.max(m, e.ts), 0);
  if (lastCrashEventTs === null) {
    lastCrashEventTs = newest;
    return;
  }
  for (const e of events) {
    if (e.ts > lastCrashEventTs && (e.kind === 'crashed' || e.kind === 'backoff')) {
      void invoke('cloud_push_notify', { serverId: e.serverId, kind: e.kind }).catch((err) => {
        // Not-signed-in is the normal case; keep it visible in dev tools.
        console.debug('[push] cloud_push_notify skipped:', err);
      });
    }
  }
  lastCrashEventTs = Math.max(lastCrashEventTs, newest);
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
    const gen = ++fetchGeneration;
    const nodeAtStart = currentNodeId();
    try {
      const servers = await invoke<Server[]>('list_servers', {
        nodeId: nodeAtStart,
      });
      // Drop a response that resolved after a node switch or a newer fetch.
      if (gen !== fetchGeneration || nodeAtStart !== currentNodeId()) return;
      set({ servers, isLoading: false });
      void checkCrashJournal();
      const selected = get().selectedServer;
      if (selected) {
        const updated = servers.find((s) => s.id === selected.id);
        if (updated) set({ selectedServer: updated });
      }
    } catch (error) {
      if (gen !== fetchGeneration) return;
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
        autoSyncToCloud();
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
      // Sub-user mode: the cmd went through the relay; the owner's executor runs it.
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
      return true;
    } catch (error) {
      // Return the outcome so a restart can abort when the stop failed.
      set({ error: String(error), isLoading: false });
      return false;
    }
  },

  deleteServer: async (serverId, deleteData = true) => {
    set({ isLoading: true, error: null });
    try {
      if (isRelayRouted()) {
        // Sub-user mode: relay the delete to the owner's machine; nothing local applies.
        await routeServerAction('server.delete', { serverId, deleteData });
        emitAudit('server.delete', serverId, { deleteData });
        const selected = get().selectedServer;
        if (selected?.id === serverId) set({ selectedServer: null });
        set({ isLoading: false });
        return;
      }
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
      tombstoneInCloud(serverId);
      autoSyncToCloud();
      set({ isLoading: false });
    } catch (error) {
      set({ error: String(error), isLoading: false });
    }
  },

  updateServerConfig: async (serverId, config) => {
    try {
      if (isRelayRouted()) {
        // Sub-user (admin): the server lives on the owner's Docker; relay the change.
        await routeServerAction('server.update_config', { serverId, config });
        emitAudit('server.update_config', serverId);
        return true;
      }
      const response = await invoke<ServerResponse>('update_server_config', {
        serverId,
        config,
        nodeId: currentNodeId(),
      });
      if (response.success) {
        emitAudit('server.update_config', serverId);
        await get().fetchServers();
        autoSyncToCloud();
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
      if (isRelayRouted()) {
        // Sub-user: relay the reinstall to the owner's machine.
        await routeServerAction('server.reinstall', { serverId });
        set({ isLoading: false });
        return;
      }
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
      if (isRelayRouted()) {
        // No relay cmd exists for a game update; say so instead of firing a doomed local invoke.
        set({
          error: "Updating the game image runs on the owner's machine — ask the owner to run it.",
          isLoading: false,
        });
        return;
      }
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
      // Truncate the command in metadata; the full text could carry sensitive args.
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
    // Generation guard: a detach/attach racing a pending listen() must not leak a duplicate listener.
    const gen = ++attachGeneration;

    try {
      // One listener for both modes: RelayLogBridge re-emits relay console lines as `server-log`.
      const unlisten = await listen<LogEvent>('server-log', (event) => {
        if (event.payload.server_id === serverId) {
          set((state) => {
            const next = [...state.logs, event.payload.line];
            return { logs: next.length > MAX_CONSOLE_LINES ? next.slice(-MAX_CONSOLE_LINES) : next };
          });
        }
      });
      if (gen !== attachGeneration) {
        unlisten(); // superseded while listen() was in flight
        return;
      }

      set({ logUnlisten: unlisten, isStreaming: true });

      // Owner: start Docker's log reader locally. Sub-user: ask the owner's executor over the relay.
      if (!isRelayRouted()) {
        await invoke('attach_server', {
          serverId,
          nodeId: currentNodeId(),
        });
      } else {
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
    attachGeneration++; // cancel any attach whose listen() is still in flight
    const { logUnlisten } = get();
    if (logUnlisten) {
      logUnlisten();
      set({ logUnlisten: null, isStreaming: false });
    }
    try {
      if (!isRelayRouted()) {
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

// Re-fetch when the active node changes; the old node's log stream is detached first.
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
