// Nodes store — tracks the local and remote LocalForge nodes the user
// has paired with. Persistence lives on the Rust side; this store just
// caches the latest snapshot.

import { create } from 'zustand';
import { invoke } from '@tauri-apps/api/core';
import type { AddRemoteNodeRequest, DockerInfo, NodeRecord } from '../types';

interface NodesState {
  nodes: NodeRecord[];
  isLoading: boolean;
  error: string | null;
  activeNodeId: string;

  fetchNodes: () => Promise<void>;
  setActiveNode: (id: string) => void;
  testRemote: (req: AddRemoteNodeRequest) => Promise<DockerInfo>;
  addRemote: (req: AddRemoteNodeRequest) => Promise<NodeRecord>;
  removeNode: (id: string) => Promise<void>;
  reconnectNode: (id: string) => Promise<void>;
  installCommand: (params: {
    domain?: string;
    label?: string;
    version?: string;
  }) => Promise<string>;
}

export const useNodesStore = create<NodesState>((set, get) => ({
  nodes: [],
  isLoading: false,
  error: null,
  activeNodeId: 'local',

  fetchNodes: async () => {
    set({ isLoading: true, error: null });
    try {
      const nodes = await invoke<NodeRecord[]>('list_nodes');
      set({ nodes, isLoading: false });
    } catch (e) {
      set({ error: String(e), isLoading: false });
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

  installCommand: async ({ domain, label, version }) =>
    invoke<string>('agent_install_command', { domain, label, version }),
}));
