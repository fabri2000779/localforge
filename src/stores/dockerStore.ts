// Docker status store. Tied to the currently-active node — switching
// nodes invalidates the cached status/info so callers refetch.

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

export const useDockerStore = create<DockerState>((set) => ({
  status: null,
  info: null,
  isChecking: false,

  checkStatus: async () => {
    set({ isChecking: true });
    try {
      const status = await invoke<DockerStatus>('check_docker_status', {
        nodeId: currentNodeId(),
      });
      set({ status, isChecking: false });
    } catch (error) {
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

// Re-fetch Docker status whenever the active node changes. Using
// nodesStore.subscribe gives us a clean unidirectional flow without
// component-side useEffects everywhere.
let lastActive = useNodesStore.getState().activeNodeId;
useNodesStore.subscribe((state) => {
  if (state.activeNodeId !== lastActive) {
    lastActive = state.activeNodeId;
    useDockerStore.setState({ status: null, info: null });
    useDockerStore.getState().checkStatus();
    useDockerStore.getState().fetchInfo();
  }
});
