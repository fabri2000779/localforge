/** Cloud auth state (optional; `me === null` is the signed-out steady state). Hydrated from the
 *  Rust `cloud_me` command and the `cloud://signed-in` deep-link event. */
import { create } from 'zustand';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { useNodesStore } from './nodesStore';

interface Subscription {
  plan: 'free' | 'hobby' | 'team';
  currentPeriodEnd: number | null;
  cancelAtPeriodEnd: boolean;
  trialEndsAt: number | null;
  /** Unix ms when cloud-side data is purged (set after dropping to free). */
  purgeAt: number | null;
}

export interface Me {
  id: string;
  email: string;
  displayName: string | null;
  emailVerifiedAt: number | null;
  createdAt: number;
  subscription: Subscription;
  /** Envelope-encryption material; null until the sync key is set up (fresh OAuth accounts). */
  syncKey: {
    wrappedDek: string;
    kekSalt: string;
    kekParams?: { algo: string; n: number; r: number; p: number; len: number } | null;
  } | null;
}

/** Drives the SyncKeyDialog: 'not_set_up' (setup), 'locked' (unlock), 'unlocked', or null when signed out. */
export type SyncKeyStatus = 'not_set_up' | 'locked' | 'unlocked' | null;

interface ApiErrorShape {
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
  loginOAuth: (provider: 'apple' | 'discord' | 'google' | 'github') => Promise<void>;
  logout: () => Promise<void>;
  refreshMe: () => Promise<void>;
  requestPasswordReset: (email: string) => Promise<boolean>;
  resendVerification: () => Promise<boolean>;
  openCheckout: (plan: 'hobby' | 'team') => Promise<boolean>;
  openPortal: () => Promise<boolean>;

  // Multi-org / sub-user mode
  /** Every org the user belongs to (own + invited). Refreshed via fetchOrgs(). */
  orgs: OrgSummary[];
  /** Active org id (persisted in localStorage). */
  currentOrgId: string | null;
  /** Caller's role in the current org. Cached so role gating is sync. */
  currentRole: OrgRole | null;
  fetchOrgs: () => Promise<void>;
  setCurrentOrg: (orgId: string) => void;
  /** (Re)connect the relay to the ACTIVE org; idempotent. */
  ensureRelay: () => void;

  // Cloud sync
  syncing: boolean;
  lastSyncedAt: number | null;
  lastSyncResult: SyncResult | null;
  syncNow: () => Promise<SyncResult | null>;
  syncPull: () => Promise<RemoteServer[] | null>;
  /** Owner-side node sync: restore nodes paired on another desktop, push the local ones. */
  syncNodes: () => Promise<void>;
  // Vault key
  vaultExportKey: () => Promise<string | null>;
  vaultImportKey: (b64: string) => Promise<boolean>;
  vaultHasKey: () => Promise<boolean>;
  /** Trigger the API export + native save dialog. Returns the chosen path or null. */
  exportData: () => Promise<string | null>;

  // Envelope-encryption sync key status + setup/unlock helpers.
  syncKeyStatus: SyncKeyStatus;
  /** Bumped to re-open the SyncKeyDialog after "Skip for now". */
  openSyncKeyTick: number;
  openSyncKeyDialog: () => void;
  refreshSyncKeyStatus: () => Promise<SyncKeyStatus>;
  /** Set up a brand-new sync key (OAuth signups picking a passphrase). */
  setupSyncKey: (secret: string) => Promise<boolean>;
  /** Unlock with the passphrase on a second device; false on a wrong secret. */
  unlockSyncKey: (secret: string) => Promise<boolean>;
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

/** Org the relay loop is pointed at ('__primary__' before orgs load); reset on sign-in/out. */
let relayOrg: string | null = null;

/** True while an org switch is mid-pivot; pulls are skipped so none runs against a half-applied org+DEK. */
let orgSwitchInFlight = false;

/** Bumped on every org pivot; a pull that resolves under a different epoch is discarded. */
let syncEpoch = 0;

/** Pivot the Rust active org and the decryption DEK together, DEK first so the push path can't
 *  fire with the wrong key in the gap. Errors are non-fatal. */
async function applyOrgScope(orgId: string, isOwner: boolean): Promise<void> {
  try {
    if (isOwner) await invoke('cloud_clear_org_dek');
    else await invoke('cloud_unlock_org_dek', { orgId });
  } catch {
    /* no grant / no keypair yet — pull shows undecrypted rows */
  }
  try {
    // isOwner tells the Rust push path whether this org is ours.
    await invoke('cloud_set_active_org', { orgId, isOwner });
  } catch {
    /* server falls back to the caller's primary org */
  }
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
      relayOrg = null;
      set({ me, loading: false });
      // Connect the relay on startup (primary org; fetchOrgs re-points it at the active org).
      if (me) {
        get().ensureRelay();
      }
      if (me) {
        void get().fetchOrgs();
        void get().refreshSyncKeyStatus();
        void invoke('cloud_claim_desktop').catch(() => {});
        void get().syncNodes();
      }
    } catch (e) {
      // Land in "not signed in" rather than a stuck loading state.
      set({ me: null, loading: false, error: asErr(e) });
    }
  },

  subscribeToEvents: async () => {
    const unSignedIn = await listen<Me>('cloud://signed-in', (event) => {
      relayOrg = null;
      set({ me: event.payload, error: null, loading: false });
      get().ensureRelay();
      void get().fetchOrgs();
      // refreshSyncKeyStatus drives the SyncKeyDialog for OAuth users without a local DEK.
      void get().refreshSyncKeyStatus();
      void invoke('cloud_claim_desktop').catch(() => {});
      void get().syncNodes();
    });
    const unPartial = await listen('cloud://signed-in-partial', () => {
      // OAuth landed but /me failed — pull fresh once so the UI catches up.
      void get().refreshMe();
    });
    const unErr = await listen<{ code: string; message: string }>('cloud://auth-error', (event) => {
      set({ error: event.payload, loading: false });
    });
    const unSync = await listen('cloud://sync-changed', () => {
      // Another device pushed: refresh the remote list.
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
      void invoke('cloud_claim_desktop').catch(() => {});
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
      void invoke('cloud_claim_desktop').catch(() => {});
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
      // loading stays true: the user is in the browser; the deep-link event flips it.
    } catch (e) {
      set({ loading: false, error: asErr(e) });
    }
  },

  logout: async () => {
    set({ loading: true });
    relayOrg = null;
    // Clear the active-org pin and borrowed DEK so nothing leaks into the next account.
    void invoke('cloud_set_active_org', { orgId: null, isOwner: true }).catch(() => {});
    void invoke('cloud_clear_org_dek').catch(() => {});
    // Disconnect the relay first so we don't keep an authed WS dangling.
    try { await invoke<void>('cloud_relay_stop'); } catch { /* ignore */ }
    try { await invoke<void>('cloud_logout'); } catch { /* ignore */ }
    set({
      me: null,
      loading: false,
      error: null,
      lastSyncResult: null,
      lastSyncedAt: null,
      syncKeyStatus: null,
      // Clear org state too, or a signed-out sub-user still reads as isSubUser until restart.
      orgs: [],
      currentOrgId: null,
      currentRole: null,
    });
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

  // Cloud sync
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

  syncNodes: async () => {
    try {
      const r = await invoke<{ imported: number }>('cloud_sync_nodes_now');
      if (r.imported > 0) await useNodesStore.getState().fetchNodes();
    } catch {
      // Signed out, locked vault or not the org owner: nothing to sync.
    }
  },

  syncPull: async () => {
    // Skip mid-pivot; setCurrentOrg pulls once the pivot completes.
    if (orgSwitchInFlight) return null;
    const epochAtStart = syncEpoch;
    try {
      const r = await invoke<RemoteServer[]>('cloud_sync_pull');
      // Discard a pull that resolved after the org pivoted.
      if (epochAtStart !== syncEpoch) return null;
      const prev = get().lastSyncResult;
      set({
        lastSyncedAt: Date.now(),
        lastSyncResult: prev
          ? { ...prev, remote: r }
          : { pushed: 0, conflicts: [], remote: r },
      });
      return r;
    } catch (e) {
      if (epochAtStart !== syncEpoch) return null;
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

  // Multi-org / sub-user mode
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
      // Stored org no longer in the list (removed, or first launch): fall back to primary.
      if (!current || !orgs.some((o) => o.id === current)) {
        current = orgs[0]?.id ?? null;
        if (current) localStorage.setItem(CURRENT_ORG_KEY, current);
      }
      const cur = orgs.find((o) => o.id === current);
      const role = cur?.role ?? null;
      set({ orgs, currentOrgId: current, currentRole: role });
      // Pin the active org + DEK before the relay/pull; bump the epoch so older pulls are discarded.
      syncEpoch++;
      if (current) await applyOrgScope(current, cur?.isOwner ?? false);
      get().ensureRelay();
      // Materialise the remote list on startup (a sub-user otherwise saw nothing until a push).
      void get().syncPull();
      // Owner: seal the org DEK to members waiting for a grant (403s are swallowed).
      for (const o of orgs) {
        if (o.isOwner) void invoke('cloud_process_grants', { orgId: o.id }).catch(() => {});
      }
    } catch (e) {
      set({ error: asErr(e) });
    }
  },

  setCurrentOrg: (orgId) => {
    const o = get().orgs.find((x) => x.id === orgId);
    if (!o) return;
    // Optimistic UI flip so the switcher highlights the new org immediately.
    localStorage.setItem(CURRENT_ORG_KEY, orgId);
    set({ currentOrgId: orgId, currentRole: o.role });
    // Gate sync until the org + DEK have pivoted; the switch pulls once itself.
    orgSwitchInFlight = true;
    syncEpoch++;
    void (async () => {
      await applyOrgScope(orgId, o.isOwner);
      orgSwitchInFlight = false;
      if (o.isOwner) void invoke('cloud_process_grants', { orgId }).catch(() => {});
      get().ensureRelay();
      void get().syncPull();
    })();
  },

  ensureRelay: () => {
    const { me, orgs, currentOrgId } = get();
    // The active-org header is owned by applyOrgScope / logout; this only manages the socket.
    if (!me) {
      relayOrg = null;
      return;
    }
    const cur = orgs.find((o) => o.id === currentOrgId);
    // An owner needs their own paid plan; a member is gated by the cloud on the host owner's Team plan.
    const allowed = cur
      ? cur.isOwner
        ? me.subscription.plan !== 'free'
        : true
      : me.subscription.plan !== 'free';
    if (!allowed) {
      if (relayOrg !== null) {
        relayOrg = null;
        void invoke('cloud_relay_stop').catch(() => {});
      }
      return;
    }
    const orgId = cur?.id;
    const target = orgId ?? '__primary__';
    if (target === relayOrg) return; // already connected there
    relayOrg = target;
    // Revert on failure so a later ensureRelay retries instead of seeing "already connected".
    void invoke('cloud_relay_start', { orgId }).catch(() => {
      if (relayOrg === target) relayOrg = null;
    });
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

  // Envelope-encryption sync key
  syncKeyStatus: null,
  openSyncKeyTick: 0,
  openSyncKeyDialog: () => set((s) => ({ openSyncKeyTick: s.openSyncKeyTick + 1 })),

  refreshSyncKeyStatus: async () => {
    try {
      const s = await invoke<SyncKeyStatus>('cloud_sync_key_status');
      set({ syncKeyStatus: s });
      return s;
    } catch {
      // Unknown on failure; don't nag about a transient blip.
      return get().syncKeyStatus;
    }
  },

  setupSyncKey: async (secret) => {
    set({ error: null });
    try {
      await invoke<void>('cloud_sync_key_setup', { secret });
      await get().refreshSyncKeyStatus();
      void get().syncNodes();
      return true;
    } catch (e) {
      set({ error: asErr(e) });
      return false;
    }
  },

  unlockSyncKey: async (secret) => {
    set({ error: null });
    try {
      await invoke<void>('cloud_sync_key_unlock', { secret });
      await get().refreshSyncKeyStatus();
      // The DEK just became available: restore any nodes paired on another desktop.
      void get().syncNodes();
      return true;
    } catch (e) {
      const ae = asErr(e);
      // wrong_secret is the expected re-prompt path, not a global error.
      if (ae.code === 'wrong_secret') return false;
      set({ error: ae });
      return false;
    }
  },
}));
