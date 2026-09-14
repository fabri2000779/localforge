/** Console log bridge (mount once): the owner forwards local `server-log` events over the relay as
 *  `console_line`; a sub-user re-emits received lines as local `server-log` events. */
import { useEffect } from 'react';
import { emit, listen } from '@tauri-apps/api/event';
import { invoke } from '@tauri-apps/api/core';
import { useAuthStore } from '../stores/authStore';

interface ServerLog {
  server_id: string;
  line: string;
  ts: number;
}

interface RelayConsoleEvent {
  type: 'event';
  kind: 'console_line';
  target: string;
  line: string;
  ts: number;
  epoch?: string;
  seq?: number;
}

export function RelayLogBridge() {
  const me = useAuthStore((s) => s.me);

  useEffect(() => {
    if (!me) return;

    let unlistenLocal: (() => void) | null = null;
    let unlistenRelay: (() => void) | null = null;
    // `cancelled` guards listen() resolutions that land after cleanup.
    let cancelled = false;

    // Owner-side forward, only when we own the active org.
    listen<ServerLog & { relayed?: boolean }>('server-log', async (event) => {
      // Skip lines we re-emitted from the relay, or two owner devices echo each other forever.
      if (event.payload?.relayed) return;
      const auth = useAuthStore.getState();
      const cur = auth.orgs.find((o) => o.id === auth.currentOrgId);
      if (!cur?.isOwner) return;
      try {
        await invoke('cloud_relay_send_event', {
          payload: {
            kind: 'console_line',
            target: event.payload.server_id,
            line: event.payload.line,
            ts: event.payload.ts,
          },
        });
      } catch {
        // relay not connected → swallow; nothing to forward to.
      }
    }).then((fn) => { if (cancelled) fn(); else unlistenLocal = fn; });

    // Sub-user-side receive: re-emit as a local server-log event.
    listen<RelayConsoleEvent>('cloud://relay-event', async (event) => {
      if (event.payload?.kind !== 'console_line') return;
      await emit('server-log', {
        server_id: event.payload.target,
        line: event.payload.line,
        ts: event.payload.ts,
        // Mark relay-sourced so the forward listener skips it.
        relayed: true,
      });
    }).then((fn) => { if (cancelled) fn(); else unlistenRelay = fn; });

    return () => {
      cancelled = true;
      if (unlistenLocal) unlistenLocal();
      if (unlistenRelay) unlistenRelay();
    };
  }, [me]);

  return null;
}
