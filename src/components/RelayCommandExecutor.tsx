/** Owner-side executor for `cloud://relay-cmd`: runs the mapped Tauri command and replies with a
 *  `cmd_result` event (errors become `{ success: false, error }`). Roles are re-checked defensively. */
import { useEffect } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { roleAtLeast, type OrgRole } from '../stores/authStore';
import { tombstoneInCloud, useServerStore } from '../stores/serverStore';
import { describeError } from '../utils/errors';

interface RelayCmd {
  type: 'cmd';
  cmd: string;
  request_id?: string;
  target?: string;
  args?: Record<string, unknown>;
  by?: { user_id: string; role: OrgRole };
}

// Relay cmd → Tauri command map; keep in sync with the role map in apps/api/src/relay.ts.
// `nodeId` routes to the right Docker host and defaults to "local".
const CMD_MAP: Record<string, { tauri: string; minRole: OrgRole; argTransform?: (cmd: RelayCmd) => Record<string, unknown> }> = {
  'server.start':         { tauri: 'start_server',  minRole: 'operator', argTransform: (c) => ({ serverId: c.target, nodeId: c.args?.nodeId ?? 'local' }) },
  'server.stop':          { tauri: 'stop_server',   minRole: 'operator', argTransform: (c) => ({ serverId: c.target, nodeId: c.args?.nodeId ?? 'local' }) },
  'server.send_command':  { tauri: 'send_command',  minRole: 'operator', argTransform: (c) => ({ serverId: c.target, nodeId: c.args?.nodeId ?? 'local', command: c.args?.command }) },
  // attach/detach drive the owner's log stream; RelayLogBridge forwards lines to the sub-user.
  'server.attach':        { tauri: 'attach_server', minRole: 'operator', argTransform: (c) => ({ serverId: c.target, nodeId: c.args?.nodeId ?? 'local' }) },
  'server.detach':        { tauri: 'detach_server', minRole: 'operator', argTransform: (c) => ({ serverId: c.target, nodeId: c.args?.nodeId ?? 'local' }) },
  'server.update_config': { tauri: 'update_server_config', minRole: 'admin', argTransform: (c) => ({ serverId: c.target, nodeId: c.args?.nodeId ?? 'local', config: c.args?.config }) },
  'server.delete':        { tauri: 'delete_server', minRole: 'admin',    argTransform: (c) => ({ serverId: c.target, nodeId: c.args?.nodeId ?? 'local', deleteData: c.args?.deleteData ?? false }) },
  'server.reinstall':     { tauri: 'reinstall_server', minRole: 'admin', argTransform: (c) => ({ serverId: c.target, nodeId: c.args?.nodeId ?? 'local' }) },
  // Backups: the S3 secret never crosses the relay; targetId defaults to the first configured target.
  'server.backup_now':     { tauri: 'cloud_backup_now',     minRole: 'operator', argTransform: (c) => ({ serverId: c.target, nodeId: c.args?.nodeId ?? 'local', targetId: c.args?.targetId ?? null }) },
  'server.restore_backup': { tauri: 'cloud_restore_backup', minRole: 'admin',    argTransform: (c) => ({ serverId: c.target, key: c.args?.key, nodeId: c.args?.nodeId ?? 'local', targetId: c.args?.targetId ?? null }) },
  'server.delete_backup':  { tauri: 'cloud_delete_backup',  minRole: 'admin',    argTransform: (c) => ({ serverId: c.target, key: c.args?.key, nodeId: c.args?.nodeId ?? 'local', targetId: c.args?.targetId ?? null }) },
  // Schedules (no secret).
  'server.upsert_schedule':{ tauri: 'upsert_schedule',      minRole: 'operator', argTransform: (c) => ({ schedule: c.args?.schedule, nodeId: c.args?.nodeId ?? 'local' }) },
  // Pass the scope-checked target so Rust verifies the schedule belongs to it.
  'server.delete_schedule':{ tauri: 'delete_schedule',      minRole: 'operator', argTransform: (c) => ({ id: c.args?.schedule_id, serverId: c.target, nodeId: c.args?.nodeId ?? 'local' }) },
};

/** Mount once at the app root; only the org owner receives cmd messages. */
export function RelayCommandExecutor() {
  useEffect(() => {
    // Runs once ([] deps): cmds only arrive while the authed relay is connected. `cancelled` guards
    // a listen() that resolves after cleanup.
    let unlisten: (() => void) | null = null;
    let cancelled = false;
    listen<RelayCmd>('cloud://relay-cmd', async (event) => {
      const msg = event.payload;
      // state.snapshot: every server with status, for the mobile's row badges (replies with its own event kind).
      if (msg.cmd === 'state.snapshot') {
        // Report the LOCAL node's servers (those are what get synced), whichever node is active.
        type SnapServer = { id: string; status: string; container_id?: string | null };
        let servers: SnapServer[];
        try {
          servers = await invoke<SnapServer[]>('list_servers', { nodeId: 'local' });
        } catch {
          servers = useServerStore.getState().servers;
        }
        try {
          await invoke('cloud_relay_send_event', {
            payload: {
              kind: 'state_snapshot',
              request_id: msg.request_id,
              servers: servers.map((s) => ({
                id: s.id,
                status: s.status,
                container_id: s.container_id ?? null,
              })),
            },
          });
        } catch (e) {
          console.error('[relay] state.snapshot reply failed', e);
        }
        return;
      }
      // server.logs: recent console backlog so a freshly opened mobile console isn't blank.
      if (msg.cmd === 'server.logs') {
        try {
          const res = await invoke<{ logs: string[] }>('get_server_logs', {
            serverId: msg.target,
            lines: typeof msg.args?.lines === 'number' ? msg.args.lines : 200,
            nodeId: (msg.args?.nodeId as string | undefined) ?? 'local',
          });
          await invoke('cloud_relay_send_event', {
            payload: {
              kind: 'logs_snapshot',
              request_id: msg.request_id,
              target: msg.target,
              lines: res.logs ?? [],
            },
          });
        } catch (e) {
          console.error('[relay] server.logs reply failed', e);
        }
        return;
      }
      // server.stats: one-shot container usage for the mobile.
      if (msg.cmd === 'server.stats') {
        try {
          const stats = await invoke('get_server_stats', {
            serverId: msg.target,
            nodeId: (msg.args?.nodeId as string | undefined) ?? 'local',
          });
          await invoke('cloud_relay_send_event', {
            payload: {
              kind: 'stats_snapshot',
              request_id: msg.request_id,
              target: msg.target,
              stats,
            },
          });
        } catch (e) {
          console.error('[relay] server.stats reply failed', e);
        }
        return;
      }
      // backups_list / schedules_list: read-only lists; a cmd_result error resolves the mobile's spinner.
      if (msg.cmd === 'server.backups_list') {
        try {
          const backups = await invoke('cloud_list_backups', {
            serverId: msg.target,
            nodeId: (msg.args?.nodeId as string | undefined) ?? 'local',
            targetId: (msg.args?.targetId as string | undefined) ?? null,
          });
          await invoke('cloud_relay_send_event', {
            payload: { kind: 'backups_snapshot', request_id: msg.request_id, target: msg.target, backups },
          });
        } catch (e) {
          respond(msg, { success: false, error: describeError(e) });
        }
        return;
      }
      if (msg.cmd === 'server.schedules_list') {
        try {
          const schedules = await invoke('list_schedules', {
            serverId: msg.target,
            nodeId: (msg.args?.nodeId as string | undefined) ?? 'local',
          });
          await invoke('cloud_relay_send_event', {
            payload: { kind: 'schedules_snapshot', request_id: msg.request_id, target: msg.target, schedules },
          });
        } catch (e) {
          respond(msg, { success: false, error: describeError(e) });
        }
        return;
      }
      // server.restart: a real stop-then-start (there is no single restart command).
      if (msg.cmd === 'server.restart') {
        if (!roleAtLeast(msg.by?.role ?? null, 'operator')) {
          return respond(msg, { success: false, error: 'forbidden' });
        }
        const nodeId = (msg.args?.nodeId as string | undefined) ?? 'local';
        try {
          await invoke('stop_server', { serverId: msg.target, nodeId });
          await invoke('start_server', { serverId: msg.target, nodeId });
          respond(msg, { success: true });
        } catch (e) {
          respond(msg, { success: false, error: describeError(e) });
        }
        return;
      }
      const handler = CMD_MAP[msg.cmd];
      if (!handler) {
        return respond(msg, { success: false, error: `unknown_cmd:${msg.cmd}` });
      }
      // Defense in depth; the relay already enforced the role.
      if (!roleAtLeast(msg.by?.role ?? null, handler.minRole)) {
        return respond(msg, { success: false, error: 'forbidden' });
      }
      try {
        const args = handler.argTransform ? handler.argTransform(msg) : {};
        await invoke(handler.tauri, args);
        respond(msg, { success: true });
        afterMutatingCmd(msg);
      } catch (e) {
        respond(msg, { success: false, error: describeError(e) });
      }
    }).then((fn) => {
      if (cancelled) fn();
      else unlisten = fn;
    });

    return () => {
      cancelled = true;
      if (unlisten) unlisten();
    };
  }, []);

  return null;
}

/** Relayed cmds that change a server's definition bypass serverStore: mirror its refresh + cloud sync
 *  (and tombstone on delete) so other devices don't keep a stale or ghost server. */
const MUTATING_CMDS = new Set(['server.update_config', 'server.reinstall', 'server.delete']);
function afterMutatingCmd(msg: RelayCmd): void {
  if (!MUTATING_CMDS.has(msg.cmd)) return;
  if (msg.cmd === 'server.delete' && msg.target) tombstoneInCloud(msg.target);
  void useServerStore.getState().fetchServers();
  void invoke('cloud_sync_now').catch(() => {
    /* not signed in / sync not set up — nothing to push */
  });
}

async function respond(
  msg: RelayCmd,
  result: { success: boolean; error?: string },
): Promise<void> {
  if (!msg.request_id) return; // fire-and-forget
  try {
    await invoke('cloud_relay_send_event', {
      payload: {
        type: 'event',
        kind: 'cmd_result',
        request_id: msg.request_id,
        cmd: msg.cmd,
        target: msg.target,
        ...result,
      },
    });
  } catch (e) {
    console.error('[relay] failed to send cmd response', e);
  }
}
