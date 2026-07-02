/**
 * Fleet discovery store — the live, cross-machine status map.
 *
 * The desktop's own servers always have authoritative status from the
 * serverStore (it talks to Docker directly). But servers that live on
 * ANOTHER machine — the owner's second desktop, an enrolled agent, or
 * (for a sub-user) the owner's machines entirely — are only reachable
 * over the cloud relay. `RelayFleetBridge` discovers them by sending a
 * `state.snapshot` cmd to every online machine and recording the reply
 * here, keyed by server id. The Servers UI then reads this map to render
 * live badges + machine grouping without decrypting anything or holding
 * a direct connection.
 *
 * Two inputs feed the store, both arriving as `cloud://relay-event`s:
 *   - `state_snapshot` (reply to our `disc:<machineId>` probe) → seeds the
 *     full per-machine status + the server→machine attribution.
 *   - `server.state_changed` (owner-side live broadcast) → patches one id.
 *
 * Everything is best-effort and self-healing: a machine that drops off the
 * relay simply stops updating; the next reconnect re-probes.
 */
import { create } from 'zustand';
import type { Server } from '../types';

export type FleetStatus = Server['status'];

/** Coerce whatever string the relay sent into our canonical status enum.
 *  Unknown values collapse to a safe default so the UI never renders a
 *  broken badge. 'crashed' passes through — it's a canonical status the UI
 *  renders distinctly (flattening it to 'error' hid crashes on remote nodes). */
export function coerceStatus(raw: unknown): FleetStatus {
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
  /** serverId → the machine id (cloud node id) it was discovered on. A
   *  cross-check / fallback for the synced `node_id`; both should agree. */
  serverMachine: Record<string, string>;

  /** Apply a `state_snapshot` reply. When `machineId` is set (the reply to
   *  a `disc:<id>` probe) we also attribute each server to that machine. */
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
