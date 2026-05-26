/**
 * Owner-side cross-machine server aggregation.
 *
 * The desktop already holds a live connection to every node it's paired
 * with — the local Docker daemon plus any remote agents added through the
 * Nodes page. So for the OWNER we can build a single unified server list
 * across all of them WITHOUT touching the cloud relay: just `list_servers`
 * per node, tagged with which machine it came from. Status is live (it's a
 * direct Docker read), and acting on a server targets its own node.
 *
 * This is what makes the desktop Servers screen match the mobile app's
 * group-by-machine view for the common case (you, the account owner, with
 * a desktop + an agent or two). Sub-users go through a different path
 * (`useDisplayedServers` + relay discovery) since they have no direct
 * connection to the owner's machines.
 *
 * Polls every 10s and whenever the node set changes; cheap because these
 * are local IPC calls, not Worker requests.
 */
import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { useNodesStore } from '../stores/nodesStore';
import type { Server } from '../types';

export interface FleetEntry {
  server: Server;
  /** The node id used to route actions + open the detail view. Matches the
   *  ids in nodesStore (`'local'` or a remote's id). */
  machineId: string;
  machineName: string;
  machineKind: 'local' | 'agent';
}

export function useOwnerFleet(enabled: boolean): {
  entries: FleetEntry[];
  loading: boolean;
  refresh: () => void;
} {
  const nodes = useNodesStore((s) => s.nodes);
  const [byNode, setByNode] = useState<Record<string, Server[]>>({});
  const [loading, setLoading] = useState(false);
  // Re-fetch when the set of nodes changes (a remote was paired/removed).
  const nodesKey = nodes.map((n) => n.id).join(',');
  const reqIdRef = useRef(0);

  const fetchAll = useCallback(async () => {
    const myReq = ++reqIdRef.current;
    setLoading(true);
    // Patch each node's list as it resolves rather than awaiting all of
    // them: an offline remote's `list_servers` can hang on its request
    // timeout, and we don't want one dead node to stall the whole view.
    await Promise.allSettled(
      nodes.map(async (n) => {
        let servers: Server[] = [];
        try {
          servers = await invoke<Server[]>('list_servers', { nodeId: n.id });
        } catch {
          servers = []; // offline / unreachable — nothing this round
        }
        if (myReq === reqIdRef.current) {
          setByNode((prev) => ({ ...prev, [n.id]: servers }));
        }
      }),
    );
    if (myReq === reqIdRef.current) setLoading(false);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [nodesKey]);

  useEffect(() => {
    if (!enabled) return;
    void fetchAll();
    const t = window.setInterval(() => void fetchAll(), 10000);
    return () => window.clearInterval(t);
  }, [enabled, fetchAll]);

  const entries = useMemo<FleetEntry[]>(() => {
    const out: FleetEntry[] = [];
    for (const n of nodes) {
      const kind: 'local' | 'agent' = n.kind.kind === 'local' ? 'local' : 'agent';
      for (const s of byNode[n.id] ?? []) {
        out.push({ server: s, machineId: n.id, machineName: n.label, machineKind: kind });
      }
    }
    return out;
  }, [nodes, byNode]);

  return { entries, loading, refresh: fetchAll };
}
