/**
 * Owner-side hook that listens for `cloud://relay-cmd` events emitted
 * by the relay WS loop, dispatches them to local Tauri commands, and
 * sends an `event` response back through the relay so the originating
 * sub-user sees their UI update.
 *
 * The relay already enforces role-vs-cmd at the wire level so by the
 * time we get here the caller has at least the required role. We
 * double-check defensively anyway.
 *
 * Failure semantics: any thrown error from the Tauri command becomes
 * `{ kind: 'cmd_result', request_id, success: false, error: <msg> }`
 * so the sub-user gets a clean failure, not a silent hang.
 */
import { useEffect } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { useAuthStore, roleAtLeast, type OrgRole } from '../stores/authStore';

interface RelayCmd {
  type: 'cmd';
  cmd: string;
  request_id?: string;
  target?: string;
  args?: Record<string, unknown>;
  by?: { user_id: string; role: OrgRole };
}

// Map relay cmds → local Tauri commands. Keep this in sync with the
// server-side role map in apps/api/src/relay.ts so the two layers
// don't drift. Anything not in this list is rejected.
const CMD_MAP: Record<string, { tauri: string; minRole: OrgRole; argTransform?: (cmd: RelayCmd) => Record<string, unknown> }> = {
  'server.start':         { tauri: 'start_server',  minRole: 'operator', argTransform: (c) => ({ serverId: c.target }) },
  'server.stop':          { tauri: 'stop_server',   minRole: 'operator', argTransform: (c) => ({ serverId: c.target }) },
  'server.restart':       { tauri: 'stop_server',   minRole: 'operator', argTransform: (c) => ({ serverId: c.target }) },
  'server.send_command':  { tauri: 'send_command',  minRole: 'operator', argTransform: (c) => ({ serverId: c.target, command: c.args?.command }) },
  'server.update_config': { tauri: 'update_server_config', minRole: 'admin', argTransform: (c) => ({ serverId: c.target, config: c.args?.config }) },
  'server.delete':        { tauri: 'delete_server', minRole: 'admin',    argTransform: (c) => ({ serverId: c.target, deleteData: c.args?.deleteData ?? false }) },
  'server.reinstall':     { tauri: 'reinstall_server', minRole: 'admin', argTransform: (c) => ({ serverId: c.target }) },
};

/**
 * Mount once at the app root. Only the OWNER of an org will receive
 * cmd messages (the relay only forwards member→owner). Sub-users
 * never see this code path.
 */
export function RelayCommandExecutor() {
  const me = useAuthStore((s) => s.me);

  useEffect(() => {
    // We don't gate on `me` here — the cmd events only arrive when the
    // relay WS is connected, which itself requires auth. If the user
    // signs out the relay loop stops emitting, so nothing fires.
    let unlisten: (() => void) | null = null;
    listen<RelayCmd>('cloud://relay-cmd', async (event) => {
      const msg = event.payload;
      const handler = CMD_MAP[msg.cmd];
      if (!handler) {
        return respond(msg, { success: false, error: `unknown_cmd:${msg.cmd}` });
      }
      // Defense in depth — the relay already enforced this, but a
      // misconfigured cloud version could in theory let something through.
      if (!roleAtLeast(msg.by?.role ?? null, handler.minRole)) {
        return respond(msg, { success: false, error: 'forbidden' });
      }
      try {
        const args = handler.argTransform ? handler.argTransform(msg) : {};
        await invoke(handler.tauri, args);
        respond(msg, { success: true });
      } catch (e) {
        respond(msg, { success: false, error: String(e) });
      }
    }).then((fn) => { unlisten = fn; });

    return () => { if (unlisten) unlisten(); };
  }, [me]);

  return null;
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
