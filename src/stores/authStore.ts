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
  /** Unix ms when our cloud-side data will be hard-deleted. Set when
   *  the user dropped to free; null while on a paid plan. */
  purgeAt: number | null;
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

  // Phase 5 — multi-org / sub-user mode
  /** Every org the user belongs to (own + invited). Refreshed via fetchOrgs(). */
  orgs: OrgSummary[];
  /** Active org id. Defaults to the user's primary org on hydrate.
   *  Switching is persisted in localStorage so it survives restart. */
  currentOrgId: string | null;
  /** Caller's role in the current org. Cached so role gating is sync. */
  currentRole: OrgRole | null;
  fetchOrgs: () => Promise<void>;
  setCurrentOrg: (orgId: string) => void;

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
  /** Trigger the API export + native save dialog. Returns the chosen path or null. */
  exportData: () => Promise<string | null>;
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

export type OrgRole = 'owner' | 'admin' | 'operator' | 'viewer';

export interface OrgSummary {
  id: string;
  name: string;
  role: OrgRole;
  isOwner: boolean;
  createdAt: number;
  joinedAt: number;
}

/** Role ranks for gating helpers. Higher = more permission. */
const ROLE_RANK: Record<OrgRole, number> = { viewer: 0, operator: 1, admin: 2, owner: 3 };
export function roleAtLeast(role: OrgRole | null | undefined, min: OrgRole): boolean {
  if (!role) return false;
  return ROLE_RANK[role] >= ROLE_RANK[min];
}

const CURRENT_ORG_KEY = 'localforge_current_org';

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
      // Auto-connect the relay on startup so this device is reachable
      // for sync pushes from elsewhere as soon as the app opens.
      if (me && me.subscription.plan !== 'free') {
        void invoke('cloud_relay_start');
      }
      // Also pull the list of orgs we belong to — needed by the
      // titlebar switcher + per-role gating across the app.
      if (me) {
        void get().fetchOrgs();
      }
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
      // Spin up the relay so other devices' edits land here instantly.
      // Free users get a 402 at upgrade time, which we silently ignore.
      if (event.payload.subscription.plan !== 'free') {
        void invoke('cloud_relay_start');
      }
      void get().fetchOrgs();
    });
    const unPartial = await listen('cloud://signed-in-partial', () => {
      // OAuth landed but /me failed — pull fresh once so the UI catches up.
      void get().refreshMe();
    });
    const unErr = await listen<{ code: string; message: string }>('cloud://auth-error', (event) => {
      set({ error: event.payload, loading: false });
    });
    // Tier 2: when the relay sees a sync_changed it auto-pulls, we just
    // patch the store so the visible "Last synced X ago" updates.
    const unSync = await listen('cloud://sync-changed', () => {
      // sync_changed comes from another device pushing — refresh our
      // remote list to reflect it. The Rust side already pulled the
      // decrypted view; we just re-read it cheaply.
      void get().syncPull();
    });
    return () => {
      unSignedIn();
      unPartial();
      unErr();
      unSync();
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
    // Disconnect the relay first so we don't keep an authed WS dangling.
    try { await invoke<void>('cloud_relay_stop'); } catch { /* ignore */ }
    try { await invoke<void>('cloud_logout'); } catch { /* ignore */ }
    set({ me: null, loading: false, error: null, lastSyncResult: null, lastSyncedAt: null });
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

  // -------------------------------------------------------------------------
  // Multi-org / sub-user mode
  // -------------------------------------------------------------------------
  orgs: [],
  currentOrgId: typeof localStorage !== 'undefined'
    ? localStorage.getItem(CURRENT_ORG_KEY)
    : null,
  currentRole: null,

  fetchOrgs: async () => {
    if (!get().me) return;
    try {
      const orgs = await invoke<OrgSummary[]>('cloud_orgs_list');
      let current = get().currentOrgId;
      // If the stored org id is no longer in the list (user got
      // removed, or first launch), fall back to primary.
      if (!current || !orgs.some((o) => o.id === current)) {
        current = orgs[0]?.id ?? null;
        if (current) localStorage.setItem(CURRENT_ORG_KEY, current);
      }
      const role = orgs.find((o) => o.id === current)?.role ?? null;
      set({ orgs, currentOrgId: current, currentRole: role });
    } catch (e) {
      set({ error: asErr(e) });
    }
  },

  setCurrentOrg: (orgId) => {
    const o = get().orgs.find((x) => x.id === orgId);
    if (!o) return;
    localStorage.setItem(CURRENT_ORG_KEY, orgId);
    set({ currentOrgId: orgId, currentRole: o.role });
    // Pull fresh sync state for the new org context.
    void get().syncPull();
  },

  exportData: async () => {
    set({ error: null });
    try {
      return await invoke<string>('cloud_export_data');
    } catch (e) {
      // User-cancelled save is not really an error; swallow.
      const ae = asErr(e);
      if (ae.code === 'decode' && ae.message === 'cancelled') return null;
      set({ error: ae });
      return null;
    }
  },
}));

// Convenience selector hooks the components can use without re-rendering
// on unrelated state changes.
export const useIsSignedIn = () => useAuthStore((s) => s.me != null && s.me !== undefined);
export const useCurrentPlan = () => useAuthStore((s) => s.me?.subscription.plan ?? 'free');
