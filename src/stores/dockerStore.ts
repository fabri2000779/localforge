// Docker status for the active node; switching nodes invalidates the cache.

import { create } from 'zustand';
import { invoke } from '@tauri-apps/api/core';
import type { DockerStatus, DockerInfo } from '../types';
import { useNodesStore } from './nodesStore';

interface DockerState {
  status: DockerStatus | null;
  info: DockerInfo | null;
  isChecking: boolean;

  checkStatus: () => Promise<void>;
  fetchInfo: () => Promise<void>;
}

const currentNodeId = () => useNodesStore.getState().activeNodeId;

// Monotonic token: a slow check from a previous node must not overwrite the current node's status.
let checkGeneration = 0;

export const useDockerStore = create<DockerState>((set) => ({
  status: null,
  info: null,
  isChecking: false,

  checkStatus: async () => {
    set({ isChecking: true });
    const gen = ++checkGeneration;
    const nodeAtStart = currentNodeId();
    try {
      const status = await invoke<DockerStatus>('check_docker_status', {
        nodeId: nodeAtStart,
      });
      if (gen !== checkGeneration || nodeAtStart !== currentNodeId()) return;
      set({ status, isChecking: false });
    } catch (error) {
      if (gen !== checkGeneration || nodeAtStart !== currentNodeId()) return;
      set({
        status: {
          available: false,
          running: false,
          error: String(error),
        },
        isChecking: false,
      });
    }
  },

  fetchInfo: async () => {
    try {
      const info = await invoke<DockerInfo>('get_docker_info', {
        nodeId: currentNodeId(),
      });
      set({ info });
    } catch (error) {
      console.error('Failed to fetch Docker info:', error);
    }
  },
}));

// Re-fetch whenever the active node changes.
let lastActive = useNodesStore.getState().activeNodeId;
useNodesStore.subscribe((state) => {
  if (state.activeNodeId !== lastActive) {
    lastActive = state.activeNodeId;
    useDockerStore.setState({ status: null, info: null });
    useDockerStore.getState().checkStatus();
    useDockerStore.getState().fetchInfo();
  }
});
