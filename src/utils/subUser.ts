/** Owner vs sub-user helpers: `useIsSubUser`, `useDisplayedServers`, `useCanAct`, and the relay
 *  routing used by the server store. */
import { invoke } from '@tauri-apps/api/core';
import { useMemo } from 'react';
import { useAuthStore, roleAtLeast, type OrgRole } from '../stores/authStore';
import { useServerStore } from '../stores/serverStore';
import { useNodesStore } from '../stores/nodesStore';
import { useFleetStore } from '../stores/fleetStore';
import type { Server } from '../types';

/** Where a displayed server actually lives, for fleet grouping + labels. */
export interface ServerMachine {
  /** The cloud node id (or synced `node_id`) the server is hosted on. */
  machineId: string;
  /** Human label — the machine's name, falling back to a short id. */
  machineName: string;
  machineKind: 'desktop' | 'agent' | 'unknown';
}

export function useIsSubUser(): boolean {
  return useAuthStore((s) => {
    const cur = s.orgs.find((o) => o.id === s.currentOrgId);
    // No org loaded yet, or signed-out → treat as "owner" (local mode).
    return cur ? !cur.isOwner : false;
  });
}

export type ActionKind =
  | 'server.start'
  | 'server.stop'
  | 'server.send_command'
  | 'server.update_config'
  | 'server.delete'
  | 'server.reinstall'
  | 'server.create'
  | 'org.invite'
  | 'org.remove_member';

/** Minimum role per action; mirrors the cloud's role map in relay.ts. */
const MIN_ROLE: Record<ActionKind, OrgRole> = {
  'server.start':         'operator',
  'server.stop':          'operator',
  'server.send_command':  'operator',
  'server.update_config': 'admin',
  'server.delete':        'admin',
  'server.reinstall':     'admin',
  'server.create':        'admin',
  'org.invite':           'admin',
  'org.remove_member':    'admin',
};

/** Whether the caller may perform `action` in the active org (true when signed out or owner). UI gating only. */
export function useCanAct(action: ActionKind): boolean {
  return useAuthStore((s) => {
    if (!s.me) return true; // local-only mode
    const cur = s.orgs.find((o) => o.id === s.currentOrgId);
    if (!cur) return true;  // before orgs hydrated; let UI render hopefully
    if (cur.isOwner) return true;
    return roleAtLeast(cur.role, MIN_ROLE[action]);
  });
}

/** Servers to render: local Docker for an owner; decrypted cloud-synced servers for a sub-user. */
export function useDisplayedServers(): Server[] {
  const local = useServerStore((s) => s.servers);
  const isSubUser = useIsSubUser();
  // Select the STABLE lastSyncResult reference; a derived `?? []` in the selector caused an update loop (#185).
  const lastSyncResult = useAuthStore((s) => s.lastSyncResult);
  // Live cross-machine status from relay discovery.
  const statuses = useFleetStore((s) => s.statuses);

  return useMemo(() => {
    if (!isSubUser) return local;
    // Map decrypted remote rows to the Server shape so existing components render unchanged.
    return (lastSyncResult?.remote ?? [])
      .filter((r) => r.decrypted)
      .map<Server>((r) => ({
        id: r.decrypted!.id,
        name: r.decrypted!.name,
        game_type: r.decrypted!.game_type as Server['game_type'],
        // 'stopped' until the hosting machine's snapshot lands.
        status: (statuses[r.decrypted!.id] ?? 'stopped') as Server['status'],
        container_id: null,
        port: r.decrypted!.port,
        memory_mb: r.decrypted!.memory_mb,
        data_path: '',
        created_at: new Date(r.updated_at).toISOString() as unknown as Server['created_at'],
        config: r.decrypted!.config,
        installed: true,
        install_container_id: null,
      } as unknown as Server));
  }, [isSubUser, local, lastSyncResult, statuses]);
}

/** Per-server machine attribution (id, name, kind) for the displayed servers; unattributable servers are absent. */
export function useServerMachineInfo(): Record<string, ServerMachine> {
  const isSubUser = useIsSubUser();
  const lastSyncResult = useAuthStore((s) => s.lastSyncResult);
  const cloudMachines = useNodesStore((s) => s.cloudMachines);
  const thisMachine = useNodesStore((s) => s.thisMachine);
  const localServers = useServerStore((s) => s.servers);
  const serverMachine = useFleetStore((s) => s.serverMachine);

  return useMemo(() => {
    const out: Record<string, ServerMachine> = {};
    const named = (id: string): ServerMachine => {
      const m = cloudMachines.find((x) => x.id === id);
      return {
        machineId: id,
        machineName: m?.name ?? (id.length > 10 ? `${id.slice(0, 8)}…` : id),
        machineKind: m?.kind ?? 'unknown',
      };
    };

    if (isSubUser) {
      for (const r of lastSyncResult?.remote ?? []) {
        if (!r.decrypted) continue;
        const nid =
          (r.decrypted as { node_id?: string }).node_id ??
          serverMachine[r.decrypted.id];
        if (nid) out[r.decrypted.id] = named(nid);
      }
      return out;
    }

    // Owner: everything in the local list lives on THIS machine.
    if (thisMachine) {
      const self: ServerMachine = {
        machineId: thisMachine.id,
        machineName: thisMachine.name,
        machineKind: 'desktop',
      };
      for (const s of localServers) out[s.id] = self;
    }
    return out;
  }, [isSubUser, lastSyncResult, cloudMachines, thisMachine, localServers, serverMachine]);
}

/** Pure route probe: would a server action go over the relay? Sends nothing (unlike routeServerAction). */
export function isRelayRouted(): boolean {
  const auth = useAuthStore.getState();
  const cur = auth.orgs.find((o) => o.id === auth.currentOrgId);
  return cur ? !cur.isOwner : false;
}

/** Route a server action: local Tauri command (owner) or a relay `cmd` (sub-user, fire-and-forget;
 *  the owner's executor answers with a `cmd_result` event). NOT a probe. */
export async function routeServerAction(
  action: ActionKind,
  payload: Record<string, unknown>,
): Promise<{ via: 'local' | 'relay'; requestId?: string }> {
  if (!isRelayRouted()) return { via: 'local' };
  const auth = useAuthStore.getState();

  // The synced config carries the hosting node_id so the owner's executor routes correctly ("local" if absent).
  const remote = auth.lastSyncResult?.remote ?? [];
  const serverId = payload.serverId as string | undefined;
  const target = remote.find((r) => r.id === serverId);
  const nodeId = (target?.decrypted as { node_id?: string } | undefined)?.node_id ?? 'local';

  // The request id correlates the cmd_result event back to the UI.
  const requestId =
    typeof crypto !== 'undefined' && 'randomUUID' in crypto
      ? crypto.randomUUID()
      : `${Date.now()}.${Math.random()}`;
  await invoke('cloud_relay_send_cmd', {
    payload: {
      type: 'cmd',
      cmd: action,
      target: payload.serverId,
      request_id: requestId,
      args: { ...payload, nodeId },
    },
  });
  return { via: 'relay', requestId };
}
