/**
 * Broadcasts owner-side server state transitions over the relay so
 * sub-users (and the mobile companion) update their per-row badges
 * instantly without polling.
 *
 * We subscribe to the serverStore, snapshot the per-server status on
 * each emission, and diff against the previous snapshot. Anything
 * that changed becomes a
 *
 *   { type: 'event', kind: 'server.state_changed', target, status }
 *
 * relay broadcast. Only the owner of the active org emits; sub-users
 * skip (they're observers, not state authorities).
 *
 * Two state sources fan into the serverStore:
 *   - User-initiated changes via the desktop UI (Start / Stop / etc.)
 *   - Periodic fetchServers polling (10s tick in App.tsx) that catches
 *     crashes and external `docker stop` from the host shell.
 *
 * That covers most cases. A truly real-time emitter would tap into
 * bollard's container-events stream directly, but the 10s tail
 * latency is acceptable for a phone glance and the polling is
 * already happening anyway.
 */
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

    // Subscribe to every serverStore mutation. Zustand calls our
    // listener synchronously after the set(), so by the time we read
    // store.servers we're seeing the new state.
    const unsub = useServerStore.subscribe((state) => {
      // Only emit when we're the owner of the org we're streaming to.
      // Sub-users' own desktops do their own server state mgmt; we
      // don't want their stops broadcasting to the owner.
      const auth = useAuthStore.getState();
      const cur = auth.orgs.find((o) => o.id === auth.currentOrgId);
      if (!cur?.isOwner) {
        // Still update our local snapshot so toggling owner role
        // later doesn't dump an instant flood of stale "changes".
        prevByIdRef.current = mapById(state.servers);
        return;
      }

      const prev = prevByIdRef.current;
      const next = mapById(state.servers);

      // Find every id whose status moved (including newly-seen ids
      // when prev didn't have them yet — that's the first-render
      // case, which we treat as no-op so the mobile doesn't get
      // spam-emitted "running → running" events on every relaunch).
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
  // Fire-and-forget. If the relay isn't connected (offline, plan
  // downgraded mid-session, etc.) the Tauri command rejects with
  // 'relay not connected' — that's fine, the next reconnect will
  // catch up via state.snapshot.
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
