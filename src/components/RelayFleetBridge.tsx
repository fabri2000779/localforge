/**
 * Owner/sub-user-side fleet discovery. Mount once at the app root.
 *
 * When the user is signed in on a paid plan (so the relay is connected),
 * this bridge:
 *   1. Enumerates the org's machines (`cloud_list_machines`) + learns which
 *      one is THIS desktop (`get_this_machine`).
 *   2. Sends a `state.snapshot` cmd tagged `disc:<machineId>` to every
 *      ONLINE machine that isn't us — the relay routes it to that exact
 *      executor (agent socket, or the specific desktop by device id).
 *   3. Records each reply (and any live `server.state_changed` broadcast)
 *      in `fleetStore`, so the Servers UI can render live badges + machine
 *      grouping for servers we can't reach with a direct Docker connection.
 *
 * We deliberately DON'T probe our own machine: the relay excludes the
 * sender from event broadcasts, so a self-probe never echoes back. Our own
 * servers' status comes from the serverStore (authoritative, direct) — see
 * `useFleetServers`.
 *
 * Re-probes on relay (re)connect and whenever the online set changes
 * (presence events refresh the machine list). Cheap: one tiny cmd per
 * online machine, only when membership actually changes.
 */
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

  // Set up the relay event plumbing whenever we're enabled / the active org
  // changes. Refetch the fleet on connect + presence so the online set the
  // probe loop reads stays current.
  useEffect(() => {
    const { clear } = useFleetStore.getState();
    if (!enabled) {
      clear();
      return;
    }
    // Fresh org context — drop stale statuses from the previous org.
    clear();
    const nodes = useNodesStore.getState();
    void nodes.fetchThisMachine();
    void nodes.fetchMachines();

    const unsubs: Array<() => void> = [];
    listen('cloud://relay-connected', () => {
      void useNodesStore.getState().fetchMachines();
    }).then((u) => unsubs.push(u));
    listen('cloud://relay-presence', () => {
      void useNodesStore.getState().fetchMachines();
    }).then((u) => unsubs.push(u));
    listen<RelaySnapshotEvent>('cloud://relay-event', (event) => {
      const msg = event.payload;
      if (!msg) return;
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
    }).then((u) => unsubs.push(u));

    return () => {
      for (const u of unsubs) u();
    };
  }, [enabled, currentOrgId]);

  // The online machines worth probing — everything except us (self-probes
  // never echo back; our own status comes from the serverStore).
  const targetKey = useMemo(() => {
    if (!enabled) return '';
    return cloudMachines
      .filter((m) => m.online && m.id !== thisMachine?.id)
      .map((m) => m.id)
      .sort()
      .join(',');
  }, [enabled, cloudMachines, thisMachine]);

  // Probe whenever that set changes (initial load, a machine comes/goes
  // online, org switch). Empty key = nothing to do.
  useEffect(() => {
    if (!targetKey) return;
    for (const id of targetKey.split(',')) probe(id);
  }, [targetKey]);

  return null;
}
