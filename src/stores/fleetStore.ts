/** Live cross-machine status map fed by relay `state_snapshot` replies and `server.state_changed`
 *  events; the Servers UI reads it for badges and machine grouping. Best-effort and self-healing. */
import { create } from 'zustand';
import type { Server } from '../types';

export type FleetStatus = Server['status'];

/** Coerce a relay status string to the canonical enum ('crashed' passes through). */
function coerceStatus(raw: unknown): FleetStatus {
  switch (raw) {
    case 'running':
    case 'stopped':
    case 'starting':
    case 'stopping':
    case 'installing':
    case 'error':
    case 'crashed':
      return raw;
    default:
      return 'stopped';
  }
}

interface FleetState {
  /** serverId → live status, across every reachable machine. */
  statuses: Record<string, FleetStatus>;
  /** serverId → machine id (cloud node id) it was discovered on. */
  serverMachine: Record<string, string>;

  /** Apply a `state_snapshot` reply; with `machineId` also attribute each server to it. */
  applySnapshot: (
    machineId: string | null,
    servers: Array<{ id: string; status?: string }>,
  ) => void;
  /** Patch a single server's status from a live `server.state_changed`. */
  applyStateChanged: (serverId: string, status: string) => void;
  /** Wipe everything — on sign-out or active-org switch. */
  clear: () => void;
}

export const useFleetStore = create<FleetState>((set) => ({
  statuses: {},
  serverMachine: {},

  applySnapshot: (machineId, servers) =>
    set((state) => {
      const statuses = { ...state.statuses };
      const serverMachine = { ...state.serverMachine };
      for (const s of servers) {
        if (!s?.id) continue;
        statuses[s.id] = coerceStatus(s.status);
        if (machineId) serverMachine[s.id] = machineId;
      }
      return { statuses, serverMachine };
    }),

  applyStateChanged: (serverId, status) =>
    set((state) => ({
      statuses: { ...state.statuses, [serverId]: coerceStatus(status) },
    })),

  clear: () => set({ statuses: {}, serverMachine: {} }),
}));
