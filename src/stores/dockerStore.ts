// Docker status store

import { create } from 'zustand';
import { invoke } from '@tauri-apps/api/core';
import type { DockerStatus, DockerInfo } from '../types';

interface DockerState {
  status: DockerStatus | null;
  info: DockerInfo | null;
  isChecking: boolean;
  
  checkStatus: () => Promise<void>;
  fetchInfo: () => Promise<void>;
}

export const useDockerStore = create<DockerState>((set) => ({
  status: null,
  info: null,
  isChecking: false,

  checkStatus: async () => {
    set({ isChecking: true });
    try {
      const status = await invoke<DockerStatus>('check_docker_status');
      set({ status, isChecking: false });
    } catch (error) {
      set({ 
        status: { 
          available: false, 
          running: false, 
          error: String(error) 
        }, 
        isChecking: false 
      });
    }
  },

  fetchInfo: async () => {
    try {
      const info = await invoke<DockerInfo>('get_docker_info');
      set({ info });
    } catch (error) {
      console.error('Failed to fetch Docker info:', error);
    }
  },
}));
