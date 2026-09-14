// Utility functions for game types

import { GameConfig } from '../types';

// Map old game type formats to new ones
const GAME_TYPE_ALIASES: Record<string, string> = {
  'minecraft_java': 'minecraft-java',
  'minecraft_bedrock': 'minecraft-bedrock',
  'seven_days_to_die': '7days',
};

/** Find a game config by type, tolerating legacy id formats. */
export function findGameConfig(games: GameConfig[], gameType: string): GameConfig | undefined {
  let config = games.find(g => g.game_type === gameType);
  if (config) return config;
  
  const aliasedType = GAME_TYPE_ALIASES[gameType];
  if (aliasedType) {
    config = games.find(g => g.game_type === aliasedType);
    if (config) return config;
  }
  
  const normalizedType = gameType.replace(/_/g, '-').toLowerCase();
  config = games.find(g => g.game_type === normalizedType);
  if (config) return config;
  
  config = games.find(g => g.game_type.replace(/-/g, '_') === gameType);
  
  return config;
}
