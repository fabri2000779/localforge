// Games configuration store

import { create } from 'zustand';
import { invoke } from '@tauri-apps/api/core';
import type { GameConfig, GameType } from '../types';

interface GamesState {
  games: GameConfig[];
  isLoading: boolean;
  error: string | null;
  configPath: string | null;
  
  fetchGames: () => Promise<void>;
  getGameConfig: (gameType: GameType) => GameConfig | undefined;
  addGame: (game: GameConfig) => Promise<boolean>;
  updateGame: (game: GameConfig) => Promise<boolean>;
  deleteGame: (gameType: GameType) => Promise<boolean>;
  exportGame: (gameType: GameType) => Promise<string | null>;
  exportAllCustomGames: () => Promise<string | null>;
  importGame: (json: string) => Promise<GameConfig | null>;
  importGames: (json: string) => Promise<GameConfig[]>;
  resetToDefaults: () => Promise<boolean>;
  fetchConfigPath: () => Promise<void>;
  clearError: () => void;
}

export const useGamesStore = create<GamesState>((set, get) => ({
  games: [],
  isLoading: false,
  error: null,
  configPath: null,

  fetchGames: async () => {
    set({ isLoading: true, error: null });
    try {
      const games = await invoke<GameConfig[]>('list_available_games');
      set({ games, isLoading: false });
    } catch (error) {
      console.error('Failed to fetch games:', error);
      set({ error: String(error), isLoading: false });
    }
  },

  getGameConfig: (gameType) => {
    return get().games.find(g => g.game_type === gameType);
  },

  addGame: async (game) => {
    set({ isLoading: true, error: null });
    try {
      await invoke<GameConfig>('add_custom_game', { game });
      await get().fetchGames();
      return true;
    } catch (error) {
      console.error('Failed to add game:', error);
      set({ error: String(error), isLoading: false });
      return false;
    }
  },

  updateGame: async (game) => {
    set({ isLoading: true, error: null });
    try {
      await invoke<GameConfig>('update_game', { game });
      await get().fetchGames();
      return true;
    } catch (error) {
      console.error('Failed to update game:', error);
      set({ error: String(error), isLoading: false });
      return false;
    }
  },

  deleteGame: async (gameType) => {
    set({ isLoading: true, error: null });
    try {
      await invoke('delete_game', { gameType });
      await get().fetchGames();
      return true;
    } catch (error) {
      console.error('Failed to delete game:', error);
      set({ error: String(error), isLoading: false });
      return false;
    }
  },

  exportGame: async (gameType) => {
    try {
      const json = await invoke<string>('export_game', { gameType });
      return json;
    } catch (error) {
      console.error('Failed to export game:', error);
      set({ error: String(error) });
      return null;
    }
  },

  exportAllCustomGames: async () => {
    try {
      const json = await invoke<string>('export_all_custom_games');
      return json;
    } catch (error) {
      console.error('Failed to export games:', error);
      set({ error: String(error) });
      return null;
    }
  },

  importGame: async (json) => {
    set({ isLoading: true, error: null });
    try {
      const game = await invoke<GameConfig>('import_game', { json });
      await get().fetchGames();
      return game;
    } catch (error) {
      console.error('Failed to import game:', error);
      set({ error: String(error), isLoading: false });
      return null;
    }
  },

  importGames: async (json) => {
    set({ isLoading: true, error: null });
    try {
      const games = await invoke<GameConfig[]>('import_games', { json });
      await get().fetchGames();
      return games;
    } catch (error) {
      console.error('Failed to import games:', error);
      set({ error: String(error), isLoading: false });
      return [];
    }
  },

  resetToDefaults: async () => {
    set({ isLoading: true, error: null });
    try {
      await invoke('reset_games_to_defaults');
      await get().fetchGames();
      return true;
    } catch (error) {
      console.error('Failed to reset games:', error);
      set({ error: String(error), isLoading: false });
      return false;
    }
  },

  fetchConfigPath: async () => {
    try {
      const path = await invoke<string>('get_games_config_path');
      set({ configPath: path });
    } catch (error) {
      console.error('Failed to get config path:', error);
    }
  },

  clearError: () => set({ error: null }),
}));
