/**
 * Helpers that abstract "am I the owner of the active org, or a sub-user?"
 *
 *   useIsSubUser()   — boolean. True when the active org is someone else's.
 *   useDisplayedServers() — returns either local Docker servers (owner)
 *                          or decrypted cloud-synced servers (sub-user).
 *   useCanAct(action) — per-role permission check for UI gating.
 *
 * The store-level server actions consult `routeServerAction()` to decide
 * whether to call the Tauri command locally or push a `cmd` through the
 * relay; the UI doesn't have to care.
 */
import { invoke } from '@tauri-apps/api/core';
import { useMemo } from 'react';
import { useAuthStore, roleAtLeast, type OrgRole } from '../stores/authStore';
import { useServerStore } from '../stores/serverStore';
import type { Server } from '../types';

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

/** Minimum role required to perform an action. Mirrors the cloud-side
 *  role map in `apps/api/src/relay.ts` so the two never drift. */
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

/**
 * Can the caller perform `action` in the active org? Returns true when:
 *   - Not signed in (local-only mode — anything goes)
 *   - Signed in as owner of the active org
 *   - Signed in as a member with sufficient role
 *
 * Use in the UI to hide / disable buttons. Defense-in-depth — the
 * server still enforces this regardless.
 */
export function useCanAct(action: ActionKind): boolean {
  return useAuthStore((s) => {
    if (!s.me) return true; // local-only mode
    const cur = s.orgs.find((o) => o.id === s.currentOrgId);
    if (!cur) return true;  // before orgs hydrated; let UI render hopefully
    if (cur.isOwner) return true;
    return roleAtLeast(cur.role, MIN_ROLE[action]);
  });
}

/**
 * The list of servers the UI should render right now.
 *   - Owner of the active org → the local Docker server list.
 *   - Sub-user → the decrypted cloud-synced server list (read-only
 *     view of the OWNER's machine).
 * Servers from the cloud that didn't decrypt are surfaced with a
 * `name + decryptError` so the user knows something's missing without
 * us silently hiding it.
 */
export function useDisplayedServers(): Server[] {
  const local = useServerStore((s) => s.servers);
  const isSubUser = useIsSubUser();
  // Select the STABLE `lastSyncResult` reference, not a derived array.
  // Returning `s.lastSyncResult?.remote ?? []` from the selector minted a
  // brand-new `[]` every render whenever there was no sync result (i.e.
  // for any owner), so useSyncExternalStore saw an ever-changing snapshot
  // and React aborted with "Maximum update depth exceeded" (#185). The
  // `?? []` now happens inside the memo, off the selector path.
  const lastSyncResult = useAuthStore((s) => s.lastSyncResult);

  return useMemo(() => {
    if (!isSubUser) return local;
    // Map decrypted remote to a Server-shaped object so the existing
    // ServerCard / list components render unchanged. Fields we don't
    // know about server-side go to safe defaults.
    return (lastSyncResult?.remote ?? [])
      .filter((r) => r.decrypted)
      .map<Server>((r) => ({
        id: r.decrypted!.id,
        name: r.decrypted!.name,
        game_type: r.decrypted!.game_type as Server['game_type'],
        status: 'stopped' as Server['status'], // we don't know remote status without a sub-fetch
        container_id: null,
        port: r.decrypted!.port,
        memory_mb: r.decrypted!.memory_mb,
        data_path: '',
        created_at: new Date(r.updated_at).toISOString() as unknown as Server['created_at'],
        config: r.decrypted!.config,
        installed: true,
        install_container_id: null,
      } as unknown as Server));
  }, [isSubUser, local, lastSyncResult]);
}

/**
 * Route a server action to either the local Tauri command (owner mode)
 * or a relay `cmd` message (sub-user mode). Mutates nothing in the
 * stores directly — callers wrap their own optimistic updates.
 *
 * The relay path is fire-and-forget; the owner's RelayCommandExecutor
 * processes the cmd and broadcasts a `cmd_result` event that the UI
 * listens to (see useRelayCmdResult hook).
 */
export async function routeServerAction(
  action: ActionKind,
  payload: Record<string, unknown>,
): Promise<{ via: 'local' | 'relay'; requestId?: string }> {
  const auth = useAuthStore.getState();
  const cur = auth.orgs.find((o) => o.id === auth.currentOrgId);
  const isSubUser = cur ? !cur.isOwner : false;

  if (!isSubUser) return { via: 'local' };

  // Look up the server's node_id from the decrypted sync result so the
  // owner's RelayCommandExecutor knows whether to hit local Docker or
  // a remote agent. Defaults to "local" if we don't have it (server
  // pushed before v0.1.13 has no node_id stamp).
  const remote = auth.lastSyncResult?.remote ?? [];
  const serverId = payload.serverId as string | undefined;
  const target = remote.find((r) => r.id === serverId);
  const nodeId = (target?.decrypted as { node_id?: string } | undefined)?.node_id ?? 'local';

  // Relay path. We invent a request id so the cmd_result event can be
  // correlated back to a UI optimistic state.
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
