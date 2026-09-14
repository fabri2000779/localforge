/** Fleet discovery (mount once): probes every online machine but ours with a `state.snapshot` cmd
 *  tagged `disc:<machineId>` and records replies + live state changes in fleetStore. */
import { useEffect, useMemo } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { useAuthStore } from '../stores/authStore';
import { useNodesStore } from '../stores/nodesStore';
import { useFleetStore } from '../stores/fleetStore';

interface RelaySnapshotEvent {
  kind?: string;
  request_id?: string;
  servers?: Array<{ id: string; status?: string }>;
  target?: string;
  status?: string;
}

function probe(machineId: string): void {
  void invoke('cloud_relay_send_cmd', {
    payload: {
      type: 'cmd',
      cmd: 'state.snapshot',
      request_id: `disc:${machineId}`,
      args: { nodeId: machineId },
    },
  }).catch(() => {
    /* relay not connected yet — the connect listener re-probes */
  });
}

export function RelayFleetBridge() {
  const meId = useAuthStore((s) => s.me?.id ?? null);
  const plan = useAuthStore((s) => s.me?.subscription.plan ?? null);
  const currentOrgId = useAuthStore((s) => s.currentOrgId);
  const cloudMachines = useNodesStore((s) => s.cloudMachines);
  const thisMachine = useNodesStore((s) => s.thisMachine);

  const enabled = !!meId && plan !== null && plan !== 'free';

  // Relay event plumbing per enabled/active-org change.
  useEffect(() => {
    const { clear } = useFleetStore.getState();
    if (!enabled) {
      clear();
      return;
    }
    clear();
    const nodes = useNodesStore.getState();
    void nodes.fetchThisMachine();
    void nodes.fetchMachines();

    const unsubs: Array<() => void> = [];
    // `cancelled` guards listen() resolutions that land after cleanup.
    let cancelled = false;
    const track = (u: () => void) => { if (cancelled) u(); else unsubs.push(u); };
    listen('cloud://relay-connected', () => {
      void useNodesStore.getState().fetchMachines();
    }).then(track);
    listen('cloud://relay-presence', () => {
      void useNodesStore.getState().fetchMachines();
      // A member (re)joined: seal pending grants if we own the org (403 otherwise).
      if (currentOrgId) {
        void invoke('cloud_process_grants', { orgId: currentOrgId }).catch(() => {});
      }
    }).then(track);
    listen<RelaySnapshotEvent>('cloud://relay-event', (event) => {
      const msg = event.payload;
      if (!msg) return;
      // The org DEK was rotated: re-acquire the current key.
      if (msg.kind === 'dek_rotated') {
        const auth = useAuthStore.getState();
        const cur = auth.orgs.find((o) => o.id === auth.currentOrgId);
        if (!cur) return;
        if (!cur.isOwner) {
          // Member: re-open the fresh grant and re-pull.
          void invoke('cloud_unlock_org_dek', { orgId: cur.id })
            .then(() => auth.syncPull())
            .catch(() => {});
        } else {
          // Owner's other device: re-pull; if nothing decrypts, prompt a re-unlock.
          void auth
            .syncPull()
            .then((remote) => {
              const r = remote ?? [];
              if (r.length > 0 && r.every((s) => !s.decrypted)) auth.openSyncKeyDialog();
            })
            .catch(() => {});
        }
        return;
      }
      const { applySnapshot, applyStateChanged } = useFleetStore.getState();
      if (msg.kind === 'state_snapshot' && Array.isArray(msg.servers)) {
        const rid = typeof msg.request_id === 'string' ? msg.request_id : '';
        const machineId = rid.startsWith('disc:') ? rid.slice('disc:'.length) : null;
        applySnapshot(machineId, msg.servers);
        return;
      }
      if (msg.kind === 'server.state_changed' && typeof msg.target === 'string') {
        applyStateChanged(msg.target, msg.status ?? 'stopped');
      }
    }).then(track);

    return () => {
      cancelled = true;
      for (const u of unsubs) u();
    };
  }, [enabled, currentOrgId]);

  // Online machines worth probing (never ourselves; self-probes don't echo).
  const targetKey = useMemo(() => {
    if (!enabled) return '';
    return cloudMachines
      .filter((m) => m.online && m.id !== thisMachine?.id)
      .map((m) => m.id)
      .sort()
      .join(',');
  }, [enabled, cloudMachines, thisMachine]);

  // Re-probe whenever that set changes.
  useEffect(() => {
    if (!targetKey) return;
    for (const id of targetKey.split(',')) probe(id);
  }, [targetKey]);

  return null;
}
