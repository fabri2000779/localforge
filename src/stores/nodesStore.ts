// Local + remote nodes the user has paired with; persistence lives on the Rust side.

import { create } from 'zustand';
import { invoke } from '@tauri-apps/api/core';
import type {
  AddRemoteNodeRequest,
  DockerInfo,
  Machine,
  NodeRecord,
  NodeStats,
  ThisMachine,
} from '../types';

export interface ClusterSummary {
  total_nodes: number;
  online_nodes: number;
  containers_running: number;
  containers_total: number;
  images: number;
}

/** Agent enrolled for direct relay control; `online` is live from the relay. */
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
  /** Latest host stats per node id (sparse). */
  nodeStats: Record<string, NodeStats>;

  fetchNodes: () => Promise<void>;
  fetchClusterSummary: () => Promise<void>;
  fetchNodeStats: (nodeId: string) => Promise<void>;
  setActiveNode: (id: string) => void;
  testRemote: (req: AddRemoteNodeRequest) => Promise<DockerInfo>;
  addRemote: (req: AddRemoteNodeRequest) => Promise<NodeRecord>;
  removeNode: (id: string) => Promise<void>;
  reconnectNode: (id: string) => Promise<void>;
  /** Rename THIS machine; the stable id never changes. */
  renameMachine: (name: string) => Promise<void>;
  /** Record that the first-run "name this machine" prompt was handled (persisted in this_machine.toml). */
  dismissMachineNamePrompt: () => Promise<void>;
  installCommand: (params: {
    domain?: string;
    label?: string;
    version?: string;
  }) => Promise<{ linux: string; windows: string }>;

  // Every machine in the org (desktops + agents), from the cloud; empty when signed out/free/offline.
  cloudMachines: Machine[];
  fetchMachines: () => Promise<void>;

  // This desktop's stable identity; `id` is the global device id the cloud/relay address it by.
  thisMachine: ThisMachine | null;
  fetchThisMachine: () => Promise<void>;

  // Cloud-relay enrollment (direct agent control without the desktop).
  cloudNodes: CloudNodeSummary[];
  fetchCloudNodes: () => Promise<void>;
  linkNodeToCloud: (nodeId: string, name: string) => Promise<CloudNodeCreated>;
  revokeCloudNode: (nodeId: string) => Promise<void>;
}

/** The local node is always present; pre-populating it avoids a "Loading…" flash and keeps the UI
 *  usable if the first fetch fails. The real list overwrites it. */
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
      set({
        nodes: nodes.length > 0 ? nodes : [LOCAL_NODE_FALLBACK],
        isLoading: false,
      });
    } catch (e) {
      // Keep what we have (at minimum the local fallback) so the UI works offline.
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
      // Offline nodes throw: drop the cached entry so the UI shows "no data", not stale numbers.
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
    // Like servers, node configs follow the owner's account (best-effort; no-op for sub-users).
    void invoke('cloud_sync_nodes_now').catch(() => {});
    await get().fetchNodes();
    return node;
  },

  removeNode: async (id: string) => {
    await invoke('remove_node', { nodeId: id });
    void invoke('cloud_sync_delete_node', { nodeId: id }).catch(() => {});
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

  dismissMachineNamePrompt: async () => {
    // Fold the returned ThisMachine straight into the store.
    const thisMachine = await invoke<ThisMachine>('set_machine_name_prompt_dismissed');
    set({ thisMachine });
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

  thisMachine: null,

  fetchThisMachine: async () => {
    try {
      const thisMachine = await invoke<ThisMachine | null>('get_this_machine');
      set({ thisMachine: thisMachine ?? null });
    } catch {
      // Local node not installed yet (Docker unreachable) — leave null.
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
