/**
 * Bridges console log lines across the cloud relay so sub-users see
 * live server output exactly like the owner does.
 *
 *   Owner side  — every `server-log` Tauri event from local Rust gets
 *                  forwarded through the relay as
 *                    { kind: 'console_line', target: <server_id>, line, ts }
 *                  Members receive it via the relay broadcast.
 *
 *   Sub-user side — listen to cloud://relay-event filtered by
 *                    kind === 'console_line'. For each, re-emit a local
 *                    `server-log` Tauri event with the same shape so
 *                    ServerDetail's existing xterm subscription works
 *                    UNCHANGED. No xterm code touched.
 *
 * Mount once at the App root next to RelayCommandExecutor. Both ends
 * always run — the owner forwards even when no sub-users are
 * connected (the DO broadcasts to zero peers, costs nothing), so we
 * don't need to track presence to decide.
 *
 * Bandwidth: a chatty Minecraft server peaks around 1-2 KB/s of
 * console output. Comfortable for the relay's per-message cost.
 */
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

    // Owner-side forward. Two conditions must be true to forward:
    //   1. The active relay is for an org WE OWN (so the receiving
    //      sub-users actually belong to our org).
    //   2. There's at least one local server-log event firing.
    // If the active org is one where WE are the sub-user, we don't
    // forward — our local server-logs belong to a different (our own)
    // workspace and have nothing to do with whoever owns the active
    // org we're currently visiting.
    listen<ServerLog & { relayed?: boolean }>('server-log', async (event) => {
      // Don't re-forward a line that we ourselves re-emitted FROM the relay
      // (see the receive side below). Without this, two owner devices in the
      // same org bounce each other's lines in an ever-amplifying loop: A
      // forwards → B receives + re-emits → B forwards → A receives → …
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
    }).then((fn) => { unlistenLocal = fn; });

    // Sub-user-side receive. Filter relay events for console_line and
    // re-emit them as local server-log events. The existing xterm
    // listener treats them indistinguishably from a local log line.
    listen<RelayConsoleEvent>('cloud://relay-event', async (event) => {
      if (event.payload?.kind !== 'console_line') return;
      // We could de-dupe by epoch+seq here; in practice the
      // owner-side filter (`only forward your OWN log lines`) means
      // we never get our own back, and sub-users receive exactly once.
      await emit('server-log', {
        server_id: event.payload.target,
        line: event.payload.line,
        ts: event.payload.ts,
        // Mark as relay-sourced so the owner-forward listener above skips it
        // (prevents the multi-owner-device echo loop).
        relayed: true,
      });
    }).then((fn) => { unlistenRelay = fn; });

    return () => {
      if (unlistenLocal) unlistenLocal();
      if (unlistenRelay) unlistenRelay();
    };
  }, [me]);

  return null;
}
