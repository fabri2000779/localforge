/**
 * Cloud auth state. Optional — the app runs perfectly without an account.
 * `me === null` is the unauthenticated state; it's the steady state for
 * users who never sign up.
 *
 * Source of truth = the Rust `cloud_me` command + the
 * `cloud://signed-in` event emitted by the deep-link handler after an
 * OAuth callback. The store re-hydrates from `cloud_me` at startup so
 * users stay signed in across app launches (the JWT lives in the OS
 * keychain — see src-tauri/src/cloud/keychain.rs).
 */
import { create } from 'zustand';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';

export interface Subscription {
  plan: 'free' | 'hobby' | 'team';
  currentPeriodEnd: number | null;
  cancelAtPeriodEnd: boolean;
  trialEndsAt: number | null;
}

export interface Me {
  id: string;
  email: string;
  displayName: string | null;
  emailVerifiedAt: number | null;
  createdAt: number;
  subscription: Subscription;
}

export interface ApiErrorShape {
  status: number;
  code: string;
  message: string | null;
}

export type AuthError = ApiErrorShape | { code: string; message: string };

interface AuthState {
  /** null = not signed in, undefined = haven't checked yet, Me = signed in. */
  me: Me | null | undefined;
  loading: boolean;
  /** Last user-facing error from a command call. UI may surface it. */
  error: AuthError | null;
  /** Hydrate from OS keychain. Call once at app startup. */
  hydrate: () => Promise<void>;
  /** Subscribe to deep-link / OAuth events. Returns an unsubscribe fn. */
  subscribeToEvents: () => Promise<() => void>;
  signupEmail: (email: string, password: string, displayName?: string) => Promise<boolean>;
  loginEmail: (email: string, password: string) => Promise<boolean>;
  loginOAuth: (provider: 'discord' | 'google' | 'github') => Promise<void>;
  logout: () => Promise<void>;
  refreshMe: () => Promise<void>;
  requestPasswordReset: (email: string) => Promise<boolean>;
  resendVerification: () => Promise<boolean>;
  openCheckout: (plan: 'hobby' | 'team') => Promise<boolean>;
  openPortal: () => Promise<boolean>;

  // Tier 1 — cloud sync
  syncing: boolean;
  lastSyncedAt: number | null;
  lastSyncResult: SyncResult | null;
  syncNow: () => Promise<SyncResult | null>;
  syncPull: () => Promise<RemoteServer[] | null>;
  // Vault key
  vaultExportKey: () => Promise<string | null>;
  vaultImportKey: (b64: string) => Promise<boolean>;
  vaultHasKey: () => Promise<boolean>;
}

export interface RemoteServer {
  id: string;
  name: string;
  updated_at: number;
  decrypted: {
    id: string;
    name: string;
    game_type: string;
    port: number;
    memory_mb: number;
    config: Record<string, string>;
  } | null;
  exists_locally: boolean;
  decrypt_error: string | null;
}

export interface SyncResult {
  pushed: number;
  conflicts: string[];
  remote: RemoteServer[];
}

function asErr(e: unknown): AuthError {
  if (e && typeof e === 'object') {
    const o = e as Record<string, unknown>;
    if (typeof o.code === 'string') {
      return {
        status: typeof o.status === 'number' ? o.status : 0,
        code: o.code,
        message: typeof o.message === 'string' ? o.message : null,
      };
    }
  }
  return { code: 'unknown', message: String(e) };
}

export const useAuthStore = create<AuthState>((set, get) => ({
  me: undefined,
  loading: false,
  error: null,

  hydrate: async () => {
    set({ loading: true, error: null });
    try {
      const me = await invoke<Me | null>('cloud_me');
      set({ me, loading: false });
    } catch (e) {
      // Network failure / token rejected → land in "not signed in" so
      // the UI shows the sign-in affordance rather than getting stuck
      // in a loading state.
      set({ me: null, loading: false, error: asErr(e) });
    }
  },

  subscribeToEvents: async () => {
    const unSignedIn = await listen<Me>('cloud://signed-in', (event) => {
      set({ me: event.payload, error: null, loading: false });
    });
    const unPartial = await listen('cloud://signed-in-partial', () => {
      // OAuth landed but /me failed — pull fresh once so the UI catches up.
      void get().refreshMe();
    });
    const unErr = await listen<{ code: string; message: string }>('cloud://auth-error', (event) => {
      set({ error: event.payload, loading: false });
    });
    return () => {
      unSignedIn();
      unPartial();
      unErr();
    };
  },

  signupEmail: async (email, password, displayName) => {
    set({ loading: true, error: null });
    try {
      const me = await invoke<Me>('cloud_signup', { email, password, displayName });
      set({ me, loading: false });
      return true;
    } catch (e) {
      set({ loading: false, error: asErr(e) });
      return false;
    }
  },

  loginEmail: async (email, password) => {
    set({ loading: true, error: null });
    try {
      const me = await invoke<Me>('cloud_login', { email, password });
      set({ me, loading: false });
      return true;
    } catch (e) {
      set({ loading: false, error: asErr(e) });
      return false;
    }
  },

  loginOAuth: async (provider) => {
    set({ loading: true, error: null });
    try {
      await invoke<void>('cloud_oauth_start', { provider });
      // We DON'T flip loading=false here — the user is now in their
      // browser. When they return, the deep-link event flips us via
      // subscribeToEvents.
    } catch (e) {
      set({ loading: false, error: asErr(e) });
    }
  },

  logout: async () => {
    set({ loading: true });
    try { await invoke<void>('cloud_logout'); } catch { /* ignore */ }
    set({ me: null, loading: false, error: null });
  },

  refreshMe: async () => {
    try {
      const me = await invoke<Me | null>('cloud_me');
      set({ me });
    } catch (e) {
      set({ error: asErr(e) });
    }
  },

  requestPasswordReset: async (email) => {
    set({ loading: true, error: null });
    try {
      await invoke<void>('cloud_request_password_reset', { email });
      set({ loading: false });
      return true;
    } catch (e) {
      set({ loading: false, error: asErr(e) });
      return false;
    }
  },

  resendVerification: async () => {
    try {
      await invoke<void>('cloud_resend_verification');
      return true;
    } catch (e) {
      set({ error: asErr(e) });
      return false;
    }
  },

  openCheckout: async (plan) => {
    set({ error: null });
    try {
      await invoke<void>('cloud_open_checkout', { plan });
      return true;
    } catch (e) {
      set({ error: asErr(e) });
      return false;
    }
  },

  openPortal: async () => {
    set({ error: null });
    try {
      await invoke<void>('cloud_open_portal');
      return true;
    } catch (e) {
      set({ error: asErr(e) });
      return false;
    }
  },

  // -------------------------------------------------------------------------
  // Cloud sync (Tier 1)
  // -------------------------------------------------------------------------
  syncing: false,
  lastSyncedAt: null,
  lastSyncResult: null,

  syncNow: async () => {
    set({ syncing: true, error: null });
    try {
      const r = await invoke<SyncResult>('cloud_sync_now');
      set({ syncing: false, lastSyncedAt: Date.now(), lastSyncResult: r });
      return r;
    } catch (e) {
      set({ syncing: false, error: asErr(e) });
      return null;
    }
  },

  syncPull: async () => {
    try {
      const r = await invoke<RemoteServer[]>('cloud_sync_pull');
      // Patch the cached SyncResult so the UI's "remote servers" list
      // updates on relay-driven pulls too.
      const prev = get().lastSyncResult;
      set({
        lastSyncedAt: Date.now(),
        lastSyncResult: prev
          ? { ...prev, remote: r }
          : { pushed: 0, conflicts: [], remote: r },
      });
      return r;
    } catch (e) {
      set({ error: asErr(e) });
      return null;
    }
  },

  vaultExportKey: async () => {
    try {
      return await invoke<string>('cloud_vault_export_key');
    } catch (e) {
      set({ error: asErr(e) });
      return null;
    }
  },

  vaultImportKey: async (b64) => {
    try {
      await invoke<void>('cloud_vault_import_key', { keyB64: b64 });
      return true;
    } catch (e) {
      set({ error: asErr(e) });
      return false;
    }
  },

  vaultHasKey: async () => {
    try {
      return await invoke<boolean>('cloud_vault_has_key');
    } catch {
      return false;
    }
  },
}));

// Convenience selector hooks the components can use without re-rendering
// on unrelated state changes.
export const useIsSignedIn = () => useAuthStore((s) => s.me != null && s.me !== undefined);
export const useCurrentPlan = () => useAuthStore((s) => s.me?.subscription.plan ?? 'free');
