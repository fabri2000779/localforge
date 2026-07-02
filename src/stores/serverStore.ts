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
import { isRelayRouted, routeServerAction } from '../utils/subUser';

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

/// Monotonic token for attach/detach ordering — see attachToServer.
let attachGeneration = 0;

/// Fire-and-forget push of the local servers to the cloud after a
/// config-changing mutation (create / config edit / delete), so the new
/// state lands on the user's other devices and the mobile app without a
/// manual "Sync now". Best-effort: `cloud_sync_now` rejects when the user
/// isn't signed in or hasn't set up sync — we swallow that. It only ever
/// pushes the LOCAL node's servers (see cloud::sync), so it's safe to call
/// regardless of which node is active.
function autoSyncToCloud() {
  void invoke('cloud_sync_now').catch(() => {
    /* not signed in / sync not set up — nothing to push */
  });
}

/// The newest crash-journal event timestamp (ms) we've already pushed for. We
/// poll the host crash JOURNAL rather than the server status, because an
/// auto-restart flips Running→Crashed→Running inside a single crash-watcher
/// tick (~1s) — a 10s status poll almost always misses that window, whereas the
/// journal records every event. Seeded on the first read so crashes that
/// predate the app opening never re-alert.
let lastCrashEventTs: number | null = null;

// Wire shape of core::CrashEvent — #[serde(rename_all = "camelCase")], so the
// fields arrive camelCase (a snake_case `server_id` here silently read
// undefined and killed every crash push; audit finding).
interface CrashJournalEvent {
  ts: number;
  serverId: string;
  /// 'crashed' | 'restarted' | 'backoff' (CrashEventKind, kebab-case).
  kind: string;
}

/// Fire a cloud crash push for any NEW 'crashed' / 'backoff' event in the host
/// journal (we skip 'restarted' — an auto-recovery needs no alert). The cloud
/// fans the ID-only push out to the org's members' phones (handy for teammates
/// without this desktop open); it's credential-gated + best-effort, and
/// `cloud_push_notify` no-ops when the user isn't signed in. Reads the LOCAL
/// node's journal regardless of the active node — remote agents own theirs.
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
    lastCrashEventTs = newest; // baseline only — don't alert for pre-existing events
    return;
  }
  for (const e of events) {
    if (e.ts > lastCrashEventTs && (e.kind === 'crashed' || e.kind === 'backoff')) {
      void invoke('cloud_push_notify', { serverId: e.serverId, kind: e.kind }).catch((err) => {
        // Not-signed-in / no-team is the normal case — but keep it visible in
        // dev tools so an arg-shape regression can't hide again.
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
    try {
      const servers = await invoke<Server[]>('list_servers', {
        nodeId: currentNodeId(),
      });
      set({ servers, isLoading: false });
      void checkCrashJournal();
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
      if (isRelayRouted()) {
        // Sub-user mode: ship the delete to the owner's machine as a relay
        // cmd (their executor runs delete_server, admin-gated). None of the
        // local calls below apply — the old unconditional detachFromServer
        // here fired a stray server.stop at the owner's LIVE server before
        // the local delete failed (audit finding).
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
      autoSyncToCloud();
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
        // Sub-user: relay the reinstall to the owner's machine. The old path
        // called attachToServer first, which fired a stray server.start at
        // the owner before the local invoke failed (audit finding).
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
        // No relay cmd exists for a game update — surface that honestly
        // instead of firing stray cmds + a doomed local invoke.
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
    // Generation guard: `listen()` resolves asynchronously, so a detach (or a
    // second attach) racing in before it lands used to leak an orphaned
    // listener that doubled every log line (audit finding). Any newer
    // attach/detach bumps the generation and the stale resolution self-drops.
    const gen = ++attachGeneration;

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
      if (gen !== attachGeneration) {
        unlisten(); // superseded while listen() was in flight
        return;
      }

      set({ logUnlisten: unlisten, isStreaming: true });

      // Route the actual "start the stream" call. Owner: local Tauri command
      // kicks Docker's log reader. Sub-user: relay cmd asks the owner's
      // RelayCommandExecutor to do the same on their hardware, and the
      // owner's RelayLogBridge forwards each line. Pure probe — the old
      // routeServerAction('server.start') call here actually STARTED the
      // owner's server in sub-user mode (audit finding).
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
      // Pure probe — the old routeServerAction('server.stop') call here
      // actually STOPPED the owner's server in sub-user mode (audit finding).
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
