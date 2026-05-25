// Nodes store — tracks the local and remote LocalForge nodes the user
// has paired with. Persistence lives on the Rust side; this store just
// caches the latest snapshot.

import { create } from 'zustand';
import { invoke } from '@tauri-apps/api/core';
import type {
  AddRemoteNodeRequest,
  DockerInfo,
  Machine,
  NodeRecord,
  NodeStats,
} from '../types';

export interface ClusterSummary {
  total_nodes: number;
  online_nodes: number;
  containers_running: number;
  containers_total: number;
  images: number;
}

/** An agent enrolled for direct cloud-relay control. `online` is live
 *  (the relay's socket set); `id` matches the local NodeRecord.id. */
export interface CloudNodeSummary {
  id: string;
  name: string;
  createdAt: number;
  lastSeenAt: number | null;
  revoked: boolean;
  online: boolean;
}

/** Result of enrolling a node — `enrollmentBlob` is shown ONCE. */
export interface CloudNodeCreated {
  node: { id: string; name: string; createdAt: number };
  enrollmentBlob: string;
  nodeToken: string;
}

interface NodesState {
  nodes: NodeRecord[];
  isLoading: boolean;
  error: string | null;
  activeNodeId: string;
  clusterSummary: ClusterSummary | null;
  /// Latest host stats per node id. Sparse — only populated for nodes
  /// the UI has actively asked about.
  nodeStats: Record<string, NodeStats>;

  fetchNodes: () => Promise<void>;
  fetchClusterSummary: () => Promise<void>;
  fetchNodeStats: (nodeId: string) => Promise<void>;
  setActiveNode: (id: string) => void;
  testRemote: (req: AddRemoteNodeRequest) => Promise<DockerInfo>;
  addRemote: (req: AddRemoteNodeRequest) => Promise<NodeRecord>;
  removeNode: (id: string) => Promise<void>;
  reconnectNode: (id: string) => Promise<void>;
  /** Rename THIS machine (the local node). Updates the persisted
   *  identity on the Rust side, then re-fetches so the label refreshes
   *  everywhere. The machine's stable id is never changed. */
  renameMachine: (name: string) => Promise<void>;
  installCommand: (params: {
    domain?: string;
    label?: string;
    version?: string;
  }) => Promise<{ linux: string; windows: string }>;

  // Every machine in the org (desktops + agents) — the cross-machine fleet
  // a sub-user (or the owner from another desktop) can see and target.
  // Sourced from the cloud, so it spans machines this install can't reach
  // directly. Empty when signed out / free / offline.
  cloudMachines: Machine[];
  fetchMachines: () => Promise<void>;

  // Cloud-relay enrollment (direct agent control without the desktop).
  cloudNodes: CloudNodeSummary[];
  fetchCloudNodes: () => Promise<void>;
  linkNodeToCloud: (nodeId: string, name: string) => Promise<CloudNodeCreated>;
  revokeCloudNode: (nodeId: string) => Promise<void>;
}

/**
 * The local Docker node is *always* present — it's the user's own
 * machine and the backend's `list_nodes` always returns at least this
 * entry. Pre-populating it in initial state means the UI doesn't flash
 * "Loading…" while the first `fetchNodes` round-trips, and if that fetch
 * ever fails the user still sees a working entry for their own box
 * instead of being stuck on a loading placeholder forever.
 *
 * The real `list_nodes` response will overwrite this array (still
 * including the local node, plus any remotes the user has paired).
 */
const LOCAL_NODE_FALLBACK: NodeRecord = {
  id: 'local',
  label: 'This machine',
  kind: { kind: 'local' },
};

export const useNodesStore = create<NodesState>((set, get) => ({
  nodes: [LOCAL_NODE_FALLBACK],
  isLoading: false,
  error: null,
  activeNodeId: 'local',
  clusterSummary: null,
  nodeStats: {},

  fetchNodes: async () => {
    set({ isLoading: true, error: null });
    try {
      const nodes = await invoke<NodeRecord[]>('list_nodes');
      // Defensive: if the backend somehow returns an empty list, fall
      // back to the local node so the dropdown is never empty.
      set({
        nodes: nodes.length > 0 ? nodes : [LOCAL_NODE_FALLBACK],
        isLoading: false,
      });
    } catch (e) {
      // Keep whatever we already have (which at minimum is the local
      // fallback) so the UI keeps working when offline.
      set({ error: String(e), isLoading: false });
    }
  },

  fetchClusterSummary: async () => {
    try {
      const summary = await invoke<ClusterSummary>('cluster_summary');
      set({ clusterSummary: summary });
    } catch (e) {
      console.error('[Store] fetchClusterSummary error:', e);
    }
  },

  fetchNodeStats: async (nodeId: string) => {
    try {
      const stats = await invoke<NodeStats>('get_node_stats', { nodeId });
      set((state) => ({
        nodeStats: { ...state.nodeStats, [nodeId]: stats },
      }));
    } catch (e) {
      // Offline / not connected nodes throw — silently drop their cached
      // entry so the UI shows "no data" rather than stale numbers.
      set((state) => {
        const { [nodeId]: _dropped, ...rest } = state.nodeStats;
        return { nodeStats: rest };
      });
      console.warn(`[Store] node_stats(${nodeId}):`, e);
    }
  },

  setActiveNode: (id: string) => set({ activeNodeId: id }),

  testRemote: async (req) =>
    invoke<DockerInfo>('test_remote_node', { req }),

  addRemote: async (req) => {
    const node = await invoke<NodeRecord>('add_remote_node', { req });
    await get().fetchNodes();
    return node;
  },

  removeNode: async (id: string) => {
    await invoke('remove_node', { nodeId: id });
    await get().fetchNodes();
  },

  reconnectNode: async (id: string) => {
    await invoke('reconnect_node', { nodeId: id });
    await get().fetchNodes();
  },

  renameMachine: async (name: string) => {
    await invoke('set_machine_name', { name });
    await get().fetchNodes();
  },

  installCommand: async ({ domain, label, version }) =>
    invoke<{ linux: string; windows: string }>('agent_install_command', {
      domain,
      label,
      version,
    }),

  cloudMachines: [],

  fetchMachines: async () => {
    try {
      const cloudMachines = await invoke<Machine[]>('cloud_list_machines');
      set({ cloudMachines });
    } catch {
      // Signed out / free / offline — nothing to enumerate.
      set({ cloudMachines: [] });
    }
  },

  cloudNodes: [],

  fetchCloudNodes: async () => {
    try {
      const cloudNodes = await invoke<CloudNodeSummary[]>('cloud_node_list');
      set({ cloudNodes });
    } catch {
      // Signed out / free plan / offline — no cloud nodes to show.
      set({ cloudNodes: [] });
    }
  },

  linkNodeToCloud: async (nodeId, name) => {
    const res = await invoke<CloudNodeCreated>('cloud_node_create', { nodeId, name });
    await get().fetchCloudNodes();
    return res;
  },

  revokeCloudNode: async (nodeId) => {
    await invoke('cloud_node_revoke', { nodeId });
    await get().fetchCloudNodes();
  },
}));
