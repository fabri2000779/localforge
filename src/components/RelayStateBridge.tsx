/** Broadcasts owner-side server status changes over the relay as `server.state_changed` by diffing
 *  serverStore snapshots (UI actions plus the 10 s poll). */
import { useEffect, useRef } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { useAuthStore } from '../stores/authStore';
import { useServerStore } from '../stores/serverStore';
import type { Server } from '../types';

export function RelayStateBridge() {
  const me = useAuthStore((s) => s.me);
  const prevByIdRef = useRef<Map<string, Server['status']>>(new Map());

  useEffect(() => {
    if (!me) return;

    const unsub = useServerStore.subscribe((state) => {
      // Only the owner of the active org emits.
      const auth = useAuthStore.getState();
      const cur = auth.orgs.find((o) => o.id === auth.currentOrgId);
      if (!cur?.isOwner) {
        // Keep the snapshot current so a later role change doesn't flood stale "changes".
        prevByIdRef.current = mapById(state.servers);
        return;
      }

      const prev = prevByIdRef.current;
      const next = mapById(state.servers);

      // First sightings are skipped so a relaunch doesn't spam unchanged states.
      for (const [id, status] of next) {
        const before = prev.get(id);
        if (before === undefined) continue; // first sighting
        if (before === status) continue;    // unchanged
        emit(id, status);
      }
      prevByIdRef.current = next;
    });

    // Seed prev so we don't emit on the very first store read.
    prevByIdRef.current = mapById(useServerStore.getState().servers);

    return () => unsub();
  }, [me]);

  return null;
}

function mapById(servers: Server[]): Map<string, Server['status']> {
  const m = new Map<string, Server['status']>();
  for (const s of servers) m.set(s.id, s.status);
  return m;
}

function emit(target: string, status: Server['status']): void {
  // Fire-and-forget; the next reconnect catches up via state.snapshot.
  invoke('cloud_relay_send_event', {
    payload: {
      kind: 'server.state_changed',
      target,
      status,
    },
  }).catch(() => {
    /* relay not connected */
  });
}
