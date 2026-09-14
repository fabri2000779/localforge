/** Owner-side unified server list across every paired node (local Docker + agents) via direct
 *  `list_servers` calls; polls every 10 s. Sub-users use `useDisplayedServers` + relay discovery. */
import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { useNodesStore } from '../stores/nodesStore';
import type { Server } from '../types';

export interface FleetEntry {
  server: Server;
  /** Node id used to route actions and open the detail view. */
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
    // Patch each node as it resolves so one dead node can't stall the view.
    await Promise.allSettled(
      nodes.map(async (n) => {
        let servers: Server[] = [];
        try {
          servers = await invoke<Server[]>('list_servers', { nodeId: n.id });
        } catch {
          servers = [];
        }
        if (myReq === reqIdRef.current) {
          setByNode((prev) => ({ ...prev, [n.id]: servers }));
        }
      }),
    );
    if (myReq === reqIdRef.current) {
      // Drop entries for unpaired nodes so byNode can't grow unbounded.
      const liveIds = new Set(nodes.map((n) => n.id));
      setByNode((prev) => {
        const next: Record<string, Server[]> = {};
        for (const k of Object.keys(prev)) if (liveIds.has(k)) next[k] = prev[k]!;
        return next;
      });
      setLoading(false);
    }
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
